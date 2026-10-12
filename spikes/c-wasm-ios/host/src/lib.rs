//! Spike C host: runs the Plugin component under wasmtime (Cranelift or Pulley)
//! and times it against the same workload compiled natively.

use std::ffi::{c_char, CString};
use std::time::Instant;

use wasmtime::Result;
use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, Store};

wasmtime::component::bindgen!({ path: "../wit", world: "plugin" });

pub const COMPONENT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/plugin.component.wasm"));

const SEARCH_PAGES: u32 = 4;
const SEARCH_ITERS: u32 = 20;
const DECRYPT_BYTES: usize = 1 << 20;
const DECRYPT_ITERS: u32 = 10;
const KEY: [u8; 16] = *b"ristle-spike-key";
const IV: [u8; 16] = *b"ristle-spike-iv!";

/// Store data. The limiter records how much linear memory the instance holds.
#[derive(Default)]
struct HostState {
    memory: MemoryLimiter,
}

/// Records linear-memory growth. Grants every request: it only measures.
#[derive(Default)]
pub struct MemoryLimiter {
    pub linear_memory_bytes: usize,
}

impl wasmtime::ResourceLimiter for MemoryLimiter {
    fn memory_growing(&mut self, current: usize, desired: usize, _max: Option<usize>) -> Result<bool> {
        self.linear_memory_bytes += desired - current;
        Ok(true)
    }

    fn table_growing(&mut self, _current: usize, _desired: usize, _max: Option<usize>) -> Result<bool> {
        Ok(true)
    }
}

/// Engine settings under test.
#[derive(Clone, Copy, Debug)]
pub struct EngineMode {
    /// Force the Pulley interpreter instead of the Cranelift JIT.
    pub pulley: bool,
    /// The iOS memory settings: no up-front reservation and no guard region.
    pub small_memory: bool,
}

impl EngineMode {
    pub fn name(self) -> String {
        format!(
            "{}{}",
            if self.pulley { "pulley" } else { "cranelift" },
            if self.small_memory { "+small-memory" } else { "+default-memory" }
        )
    }

    pub fn config(self) -> Result<Config> {
        let mut config = Config::new();
        if self.pulley {
            config.target("pulley64")?;
            config.signals_based_traps(false);
        }
        if self.small_memory {
            // iOS caps an app's virtual address space, so the default 4 GiB
            // reservation per linear memory fails with ENOMEM. Reserve nothing
            // up front and let memories grow (and move) on demand instead.
            config.memory_reservation(0);
            config.memory_guard_size(0);
            config.memory_reservation_for_growth(1 << 20);
            config.memory_may_move(true);
        }
        Ok(config)
    }
}

impl ristle::spike::host::Host for HostState {
    fn fetch_page(&mut self, page: u32) -> Vec<u8> {
        spike_guest::synthetic_page(page)
    }
}

fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

fn checksum(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf29ce484222325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x100000001b3))
}

/// Runs the workloads in the component. `pulley` forces the interpreter, and
/// with it the iOS memory settings, as on the device.
pub fn run_component(pulley: bool) -> Result<String> {
    run_component_with(EngineMode { pulley, small_memory: pulley })
}

/// Runs the workloads in the component under the given engine settings.
pub fn run_component_with(mode: EngineMode) -> Result<String> {
    let t = Instant::now();
    let engine = Engine::new(&mode.config()?)?;
    let engine_ms = ms(t);

    let t = Instant::now();
    let component = Component::new(&engine, COMPONENT)?;
    let compile_ms = ms(t);

    let t = Instant::now();
    let mut linker = Linker::<HostState>::new(&engine);
    Plugin::add_to_linker::<HostState, wasmtime::component::HasSelf<HostState>>(&mut linker, |s| s)?;
    let mut store = Store::new(&engine, HostState::default());
    store.limiter(|s| &mut s.memory);
    let plugin = Plugin::instantiate(&mut store, &component, &linker)?;
    let instantiate_ms = ms(t);
    let instance_memory = store.data().memory.linear_memory_bytes;
    let provider = plugin.ristle_spike_provider();

    let mut search_ms = Vec::new();
    let mut hits = 0;
    for _ in 0..SEARCH_ITERS {
        let t = Instant::now();
        hits = provider.call_search(&mut store, "daft", SEARCH_PAGES)?.len();
        search_ms.push(ms(t));
    }

    let data = vec![0x5au8; DECRYPT_BYTES];
    let mut decrypt_ms = Vec::new();
    let mut sum = 0;
    for _ in 0..DECRYPT_ITERS {
        let t = Instant::now();
        let out = provider.call_decrypt(&mut store, &KEY, &IV, &data)?;
        decrypt_ms.push(ms(t));
        sum = checksum(&out);
    }

    let peak_memory = store.data().memory.linear_memory_bytes;
    Ok(format!(
        "{{\"mode\":\"{}\",\"engine_ms\":{engine_ms:.2},\"compile_ms\":{compile_ms:.2},\"instantiate_ms\":{instantiate_ms:.2},\"linear_memory_after_instantiate\":{instance_memory},\"linear_memory_after_workload\":{peak_memory},\"search_hits\":{hits},\"search_ms\":{},\"decrypt_mib_ms\":{},\"decrypt_checksum\":{sum}}}",
        mode.name(),
        stats(&search_ms),
        stats(&decrypt_ms)
    ))
}

/// The same workloads compiled natively, as the baseline.
pub fn run_native() -> String {
    let mut search_ms = Vec::new();
    let mut hits = 0;
    for _ in 0..SEARCH_ITERS {
        let t = Instant::now();
        hits = spike_guest::search_pages("daft", (0..SEARCH_PAGES).map(spike_guest::synthetic_page)).len();
        search_ms.push(ms(t));
    }
    let mut decrypt_ms = Vec::new();
    let mut sum = 0;
    for _ in 0..DECRYPT_ITERS {
        let mut data = vec![0x5au8; DECRYPT_BYTES];
        let t = Instant::now();
        spike_guest::decrypt(&KEY, &IV, &mut data);
        decrypt_ms.push(ms(t));
        sum = checksum(&data);
    }
    format!(
        "{{\"mode\":\"native\",\"search_hits\":{hits},\"search_ms\":{},\"decrypt_mib_ms\":{},\"decrypt_checksum\":{sum}}}",
        stats(&search_ms),
        stats(&decrypt_ms)
    )
}

fn stats(v: &[f64]) -> String {
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let med = s[s.len() / 2];
    format!("{{\"min\":{:.3},\"median\":{:.3},\"max\":{:.3}}}", s[0], med, s[s.len() - 1])
}

/// Full report. `include_jit` is false on iOS, where JIT is forbidden.
pub fn report(include_jit: bool) -> String {
    let mut parts = vec![run_native()];
    for pulley in [true, false] {
        if !pulley && !include_jit {
            continue;
        }
        parts.push(match run_component(pulley) {
            Ok(r) => r,
            Err(e) => format!("{{\"mode\":\"{}\",\"error\":{:?}}}", if pulley { "pulley" } else { "cranelift" }, format!("{e:#}")),
        });
    }
    format!("[{}]", parts.join(","))
}

/// C entry point for the iOS test: runs native and Pulley, returns JSON.
#[no_mangle]
pub extern "C" fn ristle_spike_run() -> *mut c_char {
    CString::new(report(false)).unwrap().into_raw()
}

#[no_mangle]
pub extern "C" fn ristle_spike_free(p: *mut c_char) {
    if !p.is_null() {
        unsafe { drop(CString::from_raw(p)) };
    }
}
