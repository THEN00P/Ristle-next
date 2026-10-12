//! Spike C remainder: the SoundCloud Plugin with host HTTPS, package signing,
//! and many Plugins at once, on Windows and Android. Every function returns
//! JSON lines so the CLI, the Android app and the write-up share one format.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ed25519_dalek::SigningKey;
use ristle_pkg::{Admission, Manifest, TrustStore, VerifyError};
use ristle_spike_host::{EngineMode, MemoryLimiter};
use serde_json::{json, Value};
use wasmtime::component::{Component, Linker};
use wasmtime::{Engine, Result, Store};

wasmtime::component::bindgen!({ path: "../wit", world: "soundcloud" });

use exports::ristle::spike_sc::provider::{SearchResults, StreamSource};
use ristle::spike_sc::http::{Request, Response};

pub mod memory;

pub const COMPONENT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/sc.component.wasm"));

const QUERY: &str = "daft punk";
const LIMIT: u32 = 30;
const LIVE_SEARCHES: usize = 5;
const LIVE_RESOLVES: usize = 3;
const REPLAY_ITERS: usize = 20;
const PLUGINS: usize = 10;

pub const MODES: [EngineMode; 4] = [
    EngineMode { pulley: false, small_memory: false },
    EngineMode { pulley: false, small_memory: true },
    EngineMode { pulley: true, small_memory: false },
    EngineMode { pulley: true, small_memory: true },
];

pub fn mode_by_name(name: &str) -> Option<EngineMode> {
    MODES.into_iter().find(|m| m.name() == name)
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn stats(v: &[f64]) -> Value {
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    json!({ "min": round(s[0]), "median": round(s[s.len() / 2]), "max": round(s[s.len() - 1]) })
}

fn round(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

pub fn platform() -> Value {
    json!({
        "section": "platform",
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "cpus": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0),
    })
}

// ---------------------------------------------------------------- packages

pub fn manifest(id: &str) -> Manifest {
    Manifest {
        id: id.to_string(),
        name: "SoundCloud (spike)".to_string(),
        version: "0.0.1".to_string(),
        author: "Ristle".to_string(),
        hosts: vec!["soundcloud.com".into(), "a-v2.sndcdn.com".into(), "api-v2.soundcloud.com".into()],
        capabilities: vec!["http".into()],
    }
}

/// A throwaway publisher key. Production keys never live in source.
pub fn publisher_key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

pub fn package(id: &str, key: &SigningKey) -> Vec<u8> {
    ristle_pkg::sign(&ristle_pkg::embed_manifest(COMPONENT, &manifest(id)), key)
}

// -------------------------------------------------------------------- HTTP

type Recorded = Arc<Mutex<HashMap<String, (u16, Vec<u8>)>>>;

#[derive(Default, Clone, Copy)]
pub struct NetStats {
    pub time: Duration,
    pub requests: u32,
    pub bytes: u64,
}

/// Host HTTPS for one Plugin instance. Live requests are recorded so the same
/// calls can be replayed to measure Plugin CPU without the network.
pub struct HostHttp {
    agent: ureq::Agent,
    allowed_hosts: Vec<String>,
    replay: bool,
    recorded: Recorded,
    pub stats: NetStats,
}

impl HostHttp {
    pub fn new(allowed_hosts: Vec<String>, recorded: Recorded, replay: bool) -> Self {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            // Redirects would have to stay within declared hosts; the spike
            // does not follow them at all.
            .max_redirects(0)
            .timeout_global(Some(Duration::from_secs(30)))
            .build()
            .into();
        Self { agent, allowed_hosts, replay, recorded, stats: NetStats::default() }
    }

    fn check(&self, url: &str) -> Result<(), String> {
        let rest = url.strip_prefix("https://").ok_or("only HTTPS is allowed")?;
        let host = rest.split(['/', '?', '#', ':']).next().unwrap_or_default();
        if self.allowed_hosts.iter().any(|h| h == host) {
            Ok(())
        } else {
            Err(format!("host {host} is not declared in the manifest"))
        }
    }

    fn fetch(agent: &ureq::Agent, req: &Request) -> Result<(u16, Vec<u8>), String> {
        let mut builder = agent.get(&req.url);
        for (k, v) in &req.headers {
            builder = builder.header(k, v);
        }
        let mut resp = builder.call().map_err(|e| e.to_string())?;
        let status = resp.status().as_u16();
        let body = resp.body_mut().with_config().limit(16 << 20).read_to_vec().map_err(|e| e.to_string())?;
        Ok((status, body))
    }

    fn run(&mut self, reqs: Vec<Request>) -> Vec<Result<Response, String>> {
        let t = Instant::now();
        let results: Vec<Result<(u16, Vec<u8>), String>> = if self.replay {
            let recorded = self.recorded.lock().unwrap();
            reqs.iter()
                .map(|r| recorded.get(&r.url).cloned().ok_or_else(|| format!("not recorded: {}", r.url)))
                .collect()
        } else {
            let checks: Vec<_> = reqs.iter().map(|r| self.check(&r.url)).collect();
            let agent = &self.agent;
            let fetched: Vec<_> = std::thread::scope(|s| {
                let handles: Vec<_> = reqs
                    .iter()
                    .zip(&checks)
                    .map(|(r, c)| {
                        let c = c.clone();
                        s.spawn(move || c.and_then(|_| Self::fetch(agent, r)))
                    })
                    .collect();
                handles.into_iter().map(|h| h.join().unwrap()).collect()
            });
            let mut recorded = self.recorded.lock().unwrap();
            for (r, f) in reqs.iter().zip(&fetched) {
                if let Ok(ok) = f {
                    recorded.insert(r.url.clone(), ok.clone());
                }
            }
            fetched
        };
        self.stats.time += t.elapsed();
        self.stats.requests += reqs.len() as u32;
        results
            .into_iter()
            .map(|r| {
                r.map(|(status, body)| {
                    self.stats.bytes += body.len() as u64;
                    Response { status, body }
                })
            })
            .collect()
    }
}

struct PluginState {
    http: HostHttp,
    memory: MemoryLimiter,
}

impl ristle::spike_sc::http::Host for PluginState {
    fn get(&mut self, req: Request) -> Result<Response, String> {
        self.http.run(vec![req]).pop().unwrap()
    }

    fn get_batch(&mut self, reqs: Vec<Request>) -> Vec<Result<Response, String>> {
        self.http.run(reqs)
    }
}

fn linker(engine: &Engine) -> Result<Linker<PluginState>> {
    let mut linker = Linker::<PluginState>::new(engine);
    Soundcloud::add_to_linker::<PluginState, wasmtime::component::HasSelf<PluginState>>(&mut linker, |s| s)?;
    Ok(linker)
}

fn new_store(engine: &Engine, hosts: Vec<String>, recorded: &Recorded, replay: bool) -> Store<PluginState> {
    let mut store = Store::new(
        engine,
        PluginState { http: HostHttp::new(hosts, recorded.clone(), replay), memory: MemoryLimiter::default() },
    );
    store.limiter(|s| &mut s.memory);
    store
}

/// Wall time, time inside host HTTP, and the rest (Plugin CPU plus ABI copying).
fn timed<T>(store: &mut Store<PluginState>, call: impl FnOnce(&mut Store<PluginState>) -> T) -> (T, f64, f64, NetStats) {
    store.data_mut().http.stats = NetStats::default();
    let t = Instant::now();
    let out = call(store);
    let wall = t.elapsed();
    let net = store.data().http.stats;
    (out, ms(wall), ms(wall.saturating_sub(net.time)), net)
}

// --------------------------------------------------------------- SoundCloud

fn first_playable(results: &SearchResults) -> Option<String> {
    results.tracks.iter().find(|t| t.playable).map(|t| t.id.clone())
}

fn search(store: &mut Store<PluginState>, plugin: &Soundcloud) -> Result<SearchResults> {
    plugin.ristle_spike_sc_provider().call_search(store, QUERY, LIMIT)?.map_err(wasmtime::Error::msg)
}

fn resolve(store: &mut Store<PluginState>, plugin: &Soundcloud, id: &str) -> Result<StreamSource> {
    plugin.ristle_spike_sc_provider().call_resolve(store, id)?.map_err(wasmtime::Error::msg)
}

/// Real SoundCloud search and resolve, live then replayed, in one engine mode.
pub fn soundcloud(mode: EngineMode) -> Value {
    match soundcloud_inner(mode) {
        Ok(v) => v,
        Err(e) => json!({ "section": "soundcloud", "mode": mode.name(), "error": format!("{e:#}") }),
    }
}

fn soundcloud_inner(mode: EngineMode) -> Result<Value> {
    let package = package("spike.soundcloud", &publisher_key(1));
    let t = Instant::now();
    let verified = ristle_pkg::verify(&package).map_err(|e| wasmtime::Error::msg(format!("{e:?}")))?;
    let verify_ms = ms(t.elapsed());
    let hosts = verified.manifest.hosts.clone();

    let engine = Engine::new(&mode.config()?)?;
    let t = Instant::now();
    let component = Component::new(&engine, &package)?;
    let compile_ms = ms(t.elapsed());
    let linker = linker(&engine)?;
    let pre = SoundcloudPre::new(linker.instantiate_pre(&component)?)?;

    let recorded: Recorded = Default::default();
    let mut store = new_store(&engine, hosts.clone(), &recorded, false);
    let t = Instant::now();
    let plugin = pre.instantiate(&mut store)?;
    let instantiate_ms = ms(t.elapsed());
    let memory_after_instantiate = store.data().memory.linear_memory_bytes;

    // Live. The first search also discovers the client id.
    let (cold, cold_wall, cold_cpu, cold_net) = timed(&mut store, |s| search(s, &plugin));
    let cold = cold?;
    let track = first_playable(&cold).ok_or_else(|| wasmtime::Error::msg("no playable track"))?;
    let mut live_search = (vec![], vec![], vec![]);
    for _ in 0..LIVE_SEARCHES {
        let (r, wall, cpu, net) = timed(&mut store, |s| search(s, &plugin));
        r?;
        live_search.0.push(wall);
        live_search.1.push(ms(net.time));
        live_search.2.push(cpu);
    }
    let mut live_resolve = (vec![], vec![], vec![]);
    let mut stream = None;
    for _ in 0..LIVE_RESOLVES {
        let (r, wall, cpu, net) = timed(&mut store, |s| resolve(s, &plugin, &track));
        stream = Some(r?);
        live_resolve.0.push(wall);
        live_resolve.1.push(ms(net.time));
        live_resolve.2.push(cpu);
    }
    let stream = stream.unwrap();
    let playlist_ok = check_stream(&stream.url);
    let memory_after_calls = store.data().memory.linear_memory_bytes;

    // Replay: the same calls without the network, for steady Plugin CPU.
    let mut replay_store = new_store(&engine, hosts.clone(), &recorded, true);
    let replay_plugin = pre.instantiate(&mut replay_store)?;
    let (r, cold_replay_wall, _, _) = timed(&mut replay_store, |s| search(s, &replay_plugin));
    r?;
    let mut replay_search = vec![];
    let mut replay_resolve = vec![];
    for _ in 0..REPLAY_ITERS {
        let (r, _, cpu, _) = timed(&mut replay_store, |s| search(s, &replay_plugin));
        r?;
        replay_search.push(cpu);
        let (r, _, cpu, _) = timed(&mut replay_store, |s| resolve(s, &replay_plugin, &track));
        r?;
        replay_resolve.push(cpu);
    }

    let native = native_baseline(&recorded, &track);

    Ok(json!({
        "section": "soundcloud",
        "mode": mode.name(),
        "package_bytes": package.len(),
        "verify_ms": round(verify_ms),
        "compile_ms": round(compile_ms),
        "instantiate_ms": round(instantiate_ms),
        "linear_memory_after_instantiate": memory_after_instantiate,
        "linear_memory_after_calls": memory_after_calls,
        "query": QUERY,
        "results": { "tracks": cold.tracks.len(), "artists": cold.artists.len(), "playlists": cold.playlists.len() },
        "first_track": cold.tracks.first().map(|t| format!("{} - {}", t.artist, t.title)),
        "resolved_track": track,
        "stream": { "mime": stream.mime_type, "host": stream.url.split('/').nth(2), "playlist_ok": playlist_ok },
        "cold_search": { "wall_ms": round(cold_wall), "network_ms": round(ms(cold_net.time)), "plugin_ms": round(cold_cpu), "requests": cold_net.requests, "bytes": cold_net.bytes },
        "search": { "wall_ms": stats(&live_search.0), "network_ms": stats(&live_search.1), "plugin_ms": stats(&live_search.2) },
        "resolve": { "wall_ms": stats(&live_resolve.0), "network_ms": stats(&live_resolve.1), "plugin_ms": stats(&live_resolve.2) },
        "replay": {
            "cold_search_incl_client_id_scan_ms": round(cold_replay_wall),
            "search_plugin_ms": stats(&replay_search),
            "resolve_plugin_ms": stats(&replay_resolve),
        },
        "native": native,
    }))
}

/// The host fetches the resolved HLS playlist to prove the source is real.
fn check_stream(url: &str) -> bool {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(20))).build().into();
    agent
        .get(url)
        .call()
        .ok()
        .and_then(|mut r| r.body_mut().read_to_vec().ok())
        .is_some_and(|b| b.starts_with(b"#EXTM3U"))
}

struct NativeReplay<'a>(&'a HashMap<String, (u16, Vec<u8>)>);

impl sc_plugin::Http for NativeReplay<'_> {
    fn get_batch(&self, requests: Vec<sc_plugin::Request>) -> Vec<Result<sc_plugin::Response, String>> {
        requests
            .into_iter()
            .map(|r| {
                self.0
                    .get(&r.url)
                    .map(|(status, body)| sc_plugin::Response { status: *status, body: body.clone() })
                    .ok_or_else(|| format!("not recorded: {}", r.url))
            })
            .collect()
    }
}

/// The same Plugin code compiled natively, over the recorded responses.
fn native_baseline(recorded: &Recorded, track: &str) -> Value {
    let recorded = recorded.lock().unwrap();
    let http = NativeReplay(&recorded);
    let plugin = sc_plugin::Plugin::default();
    let t = Instant::now();
    let ok = plugin.search(&http, QUERY, LIMIT).is_ok();
    let cold = ms(t.elapsed());
    let mut search = vec![];
    let mut resolve = vec![];
    for _ in 0..REPLAY_ITERS {
        let t = Instant::now();
        plugin.search(&http, QUERY, LIMIT).unwrap();
        search.push(ms(t.elapsed()));
        let t = Instant::now();
        plugin.resolve(&http, track).unwrap();
        resolve.push(ms(t.elapsed()));
    }
    json!({ "ok": ok, "cold_search_incl_client_id_scan_ms": round(cold), "search_ms": stats(&search), "resolve_ms": stats(&resolve) })
}

// ----------------------------------------------------------------- signing

pub fn signing() -> Value {
    let key_a = publisher_key(1);
    let key_b = publisher_key(2);
    let signed = package("spike.soundcloud", &key_a);

    let mut verify_ms = vec![];
    for _ in 0..50 {
        let t = Instant::now();
        ristle_pkg::verify(&signed).unwrap();
        verify_ms.push(ms(t.elapsed()));
    }
    // Flip one byte inside the core module's code, and one in the manifest.
    let mut code_tampered = signed.clone();
    code_tampered[COMPONENT.len() / 2] ^= 0x01;
    let manifest_at = find(&signed, b"api-v2.soundcloud.com").unwrap();
    let mut manifest_tampered = signed.clone();
    manifest_tampered[manifest_at] = b'x';
    let mut appended = signed.clone();
    ristle_pkg::push_custom_section(&mut appended, "ristle-icon", b"not covered");

    let mut trust = TrustStore::default();
    let installed = ristle_pkg::verify(&signed).unwrap();
    let first = trust.admission(&installed);
    trust.install(&installed);
    let update = trust.admission(&ristle_pkg::verify(&package("spike.soundcloud", &key_a)).unwrap());
    let takeover = trust.admission(&ristle_pkg::verify(&package("spike.soundcloud", &key_b)).unwrap());

    let engine = Engine::default();
    let wasmtime_accepts_signed = Component::new(&engine, &signed).is_ok();

    json!({
        "section": "signing",
        "scheme": "ristle-pkg (manifest + Ed25519 over SHA-256, signature section last)",
        "package_bytes": signed.len(),
        "overhead_bytes": signed.len() - COMPONENT.len(),
        "verify_ms": stats(&verify_ms),
        "untampered": label(&ristle_pkg::verify(&signed).map(|_| ())),
        "code_byte_flipped": label(&ristle_pkg::verify(&code_tampered).map(|_| ())),
        "manifest_host_changed": label(&ristle_pkg::verify(&manifest_tampered).map(|_| ())),
        "section_appended_after_signature": label(&ristle_pkg::verify(&appended).map(|_| ())),
        "first_install": format!("{first:?}"),
        "same_key_update": format!("{update:?}"),
        "other_key_same_id": match takeover { Admission::Takeover { .. } => "Takeover (rejected unless the user accepts)".to_string(), other => format!("{other:?}") },
        "wasmtime_accepts_signed_package": wasmtime_accepts_signed,
        "wasmsign2": wasmsign2_check(&engine),
    })
}

fn label(r: &std::result::Result<(), VerifyError>) -> String {
    match r {
        Ok(()) => "accepted".to_string(),
        Err(e) => format!("rejected: {e:?}"),
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// The same checks with wasmsign2, the prior art named in the requirements.
fn wasmsign2_check(engine: &Engine) -> Value {
    let run = || -> std::result::Result<Value, wasmsign2::WSError> {
        let a = wasmsign2::KeyPair::generate();
        let b = wasmsign2::KeyPair::generate();
        let unsigned = ristle_pkg::embed_manifest(COMPONENT, &manifest("spike.soundcloud"));
        let module = wasmsign2::Module::deserialize(&mut &unsigned[..])?;
        let t = Instant::now();
        let signed_module = a.sk.sign(module, None)?;
        let sign_ms = ms(t.elapsed());
        let mut signed = vec![];
        signed_module.serialize(&mut signed)?;
        let mut verify_ms = vec![];
        for _ in 0..50 {
            let t = Instant::now();
            a.pk.verify(&mut &signed[..], None)?;
            verify_ms.push(ms(t.elapsed()));
        }
        let mut tampered = signed.clone();
        let at = signed.len() - COMPONENT.len() / 2;
        tampered[at] ^= 0x01;
        Ok(json!({
            "overhead_bytes": signed.len() - unsigned.len(),
            "sign_ms": round(sign_ms),
            "verify_ms": stats(&verify_ms),
            "tampered": match a.pk.verify(&mut &tampered[..], None) { Ok(()) => "accepted".to_string(), Err(e) => format!("rejected: {e}") },
            "other_key": match b.pk.verify(&mut &signed[..], None) { Ok(()) => "accepted".to_string(), Err(e) => format!("rejected: {e}") },
            "wasmtime_accepts_signed_component": Component::new(engine, &signed).is_ok(),
        }))
    };
    run().unwrap_or_else(|e| json!({ "error": e.to_string() }))
}

// ------------------------------------------------------------ ten Plugins

/// Ten distinct Plugin Packages (different ids, so different bytes), each
/// verified, compiled and instantiated in its own Store, then called once.
pub fn many_plugins(mode: EngineMode, recorded: Option<&Recorded>) -> Value {
    match many_plugins_inner(mode, recorded) {
        Ok(v) => v,
        Err(e) => json!({ "section": "ten_plugins", "mode": mode.name(), "error": format!("{e:#}") }),
    }
}

fn many_plugins_inner(mode: EngineMode, recorded: Option<&Recorded>) -> Result<Value> {
    let key = publisher_key(1);
    let packages: Vec<Vec<u8>> = (0..PLUGINS).map(|i| package(&format!("spike.plugin{i}"), &key)).collect();
    let engine = Engine::new(&mode.config()?)?;
    let linker = linker(&engine)?;
    let m0 = memory::snapshot();

    let mut verify_ms = vec![];
    let mut compile_ms = vec![];
    let mut pres = vec![];
    let mut hosts = vec![];
    for p in &packages {
        let t = Instant::now();
        let v = ristle_pkg::verify(p).map_err(|e| wasmtime::Error::msg(format!("{e:?}")))?;
        verify_ms.push(ms(t.elapsed()));
        hosts.push(v.manifest.hosts);
        let t = Instant::now();
        let c = Component::new(&engine, p)?;
        compile_ms.push(ms(t.elapsed()));
        pres.push(SoundcloudPre::new(linker.instantiate_pre(&c)?)?);
    }
    let m1 = memory::snapshot();

    let empty = Recorded::default();
    let recorded = recorded.unwrap_or(&empty);
    let mut instantiate_ms = vec![];
    let mut instances = vec![];
    for (pre, h) in pres.iter().zip(&hosts) {
        let mut store = new_store(&engine, h.clone(), recorded, true);
        let t = Instant::now();
        let plugin = pre.instantiate(&mut store)?;
        instantiate_ms.push(ms(t.elapsed()));
        instances.push((store, plugin));
    }
    let m2 = memory::snapshot();
    let linear_after_instantiate: usize = instances.iter().map(|(s, _)| s.data().memory.linear_memory_bytes).sum();

    // One replayed search in each, so each instance holds realistic state.
    let called = !recorded.lock().unwrap().is_empty();
    if called {
        for (store, plugin) in &mut instances {
            search(store, plugin)?;
        }
    }
    let m3 = memory::snapshot();
    let linear_after_call: usize = instances.iter().map(|(s, _)| s.data().memory.linear_memory_bytes).sum();

    // A compiled-artifact cache, as the production host would keep.
    let c = Component::new(&engine, &packages[0])?;
    let t = Instant::now();
    let bytes = c.serialize()?;
    let serialize_ms = ms(t.elapsed());
    let t = Instant::now();
    // SAFETY: the bytes were produced by this engine in this process.
    let cached = unsafe { Component::deserialize(&engine, &bytes)? };
    let deserialize_ms = ms(t.elapsed());
    // Run the deserialized code, so executing cached code is proven too.
    let mut store = new_store(&engine, hosts[0].clone(), recorded, true);
    let plugin = SoundcloudPre::new(linker.instantiate_pre(&cached)?)?.instantiate(&mut store)?;
    let cached_runs = !called || search(&mut store, &plugin).is_ok();

    let per = |a: &memory::Snapshot, b: &memory::Snapshot| b.per_plugin(a, PLUGINS);
    Ok(json!({
        "section": "ten_plugins",
        "mode": mode.name(),
        "plugins": PLUGINS,
        "verify_ms": stats(&verify_ms),
        "compile_ms": stats(&compile_ms),
        "instantiate_ms": stats(&instantiate_ms),
        "linear_memory_per_instance": { "after_instantiate": linear_after_instantiate / PLUGINS, "after_search": if called { json!(linear_after_call / PLUGINS) } else { Value::Null } },
        "process_memory_per_plugin": {
            "compiled_code": per(&m0, &m1),
            "instance": per(&m1, &m2),
            "after_search": if called { per(&m2, &m3) } else { Value::Null },
        },
        "process_memory_total": { "before": m0.to_json(), "after": m3.to_json() },
        "compiled_cache": { "bytes": bytes.len(), "serialize_ms": round(serialize_ms), "deserialize_ms": round(deserialize_ms), "deserialized_component_runs": cached_runs },
    }))
}

// ------------------------------------------------------------------ driver

/// Runs the named sections (`synthetic`, `soundcloud`, `signing`, `ten`) in
/// the given modes and returns one JSON line per result.
pub fn run(sections: &[&str], modes: &[EngineMode]) -> Vec<String> {
    let mut out = vec![platform().to_string()];
    let all = sections.is_empty();
    let want = |s: &str| all || sections.contains(&s);
    if want("synthetic") {
        out.push(json!({ "section": "synthetic", "result": serde_json::from_str::<Value>(&ristle_spike_host::run_native()).unwrap() }).to_string());
        for &mode in modes {
            let r = ristle_spike_host::run_component_with(mode)
                .map(|s| serde_json::from_str::<Value>(&s).unwrap())
                .unwrap_or_else(|e| json!({ "mode": mode.name(), "error": format!("{e:#}") }));
            out.push(json!({ "section": "synthetic", "result": r }).to_string());
        }
    }
    if want("signing") {
        out.push(signing().to_string());
    }
    let mut recorded = None;
    if want("soundcloud") || want("ten") {
        for &mode in modes {
            if want("soundcloud") {
                out.push(soundcloud(mode).to_string());
            }
        }
        if want("ten") {
            recorded = Some(record_search());
        }
    }
    if want("ten") {
        for &mode in modes {
            out.push(many_plugins(mode, recorded.as_ref()).to_string());
        }
    }
    out
}

/// One live search with the Cranelift engine, recorded for replay.
fn record_search() -> Recorded {
    let recorded = Recorded::default();
    let run = || -> Result<()> {
        let engine = Engine::default();
        let component = Component::new(&engine, COMPONENT)?;
        let mut store = new_store(&engine, manifest("x").hosts, &recorded, false);
        let plugin = Soundcloud::instantiate(&mut store, &component, &linker(&engine)?)?;
        search(&mut store, &plugin)?;
        Ok(())
    };
    if let Err(e) = run() {
        eprintln!("recording failed, ten-plugin test runs without a call: {e:#}");
        recorded.lock().unwrap().clear();
    }
    recorded
}

#[cfg(target_os = "android")]
mod android {
    use jni::objects::{JClass, JString};
    use jni::sys::jstring;
    use jni::JNIEnv;

    /// `SpikeActivity.run(sections, modes)`: comma-separated names, empty for all.
    #[no_mangle]
    pub extern "system" fn Java_org_ristle_spike_SpikeActivity_run<'a>(
        mut env: JNIEnv<'a>,
        _class: JClass<'a>,
        sections: JString<'a>,
        modes: JString<'a>,
    ) -> jstring {
        let sections: String = env.get_string(&sections).map(Into::into).unwrap_or_default();
        let modes: String = env.get_string(&modes).map(Into::into).unwrap_or_default();
        let lines = super::run_from_args(&sections, &modes).join("\n");
        env.new_string(lines).expect("jstring").into_raw()
    }
}

/// Parses comma-separated section and mode lists (empty means all).
pub fn run_from_args(sections: &str, modes: &str) -> Vec<String> {
    let sections: Vec<&str> = sections.split(',').filter(|s| !s.is_empty()).collect();
    let modes: Vec<EngineMode> = if modes.is_empty() {
        MODES.to_vec()
    } else {
        modes.split(',').filter_map(mode_by_name).collect()
    };
    run(&sections, &modes)
}
