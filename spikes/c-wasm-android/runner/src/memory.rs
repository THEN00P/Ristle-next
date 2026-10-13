//! Process memory: resident/private bytes and reserved virtual address space.

use serde_json::{json, Value};

#[derive(Clone, Copy, Default)]
pub struct Snapshot {
    /// Windows: private commit. Linux/Android: VmRSS.
    pub resident: u64,
    /// Virtual address space in use (reservations included).
    pub virtual_bytes: u64,
}

impl Snapshot {
    pub fn to_json(&self) -> Value {
        json!({ "resident_mib": mib(self.resident), "virtual_gib": gib(self.virtual_bytes) })
    }

    pub fn per_plugin(&self, before: &Snapshot, n: usize) -> Value {
        let d = |a: u64, b: u64| a.saturating_sub(b) as f64 / n as f64;
        json!({
            "resident_kib": (d(self.resident, before.resident) / 1024.0).round(),
            "virtual_mib": (d(self.virtual_bytes, before.virtual_bytes) / (1024.0 * 1024.0)).round(),
        })
    }
}

fn mib(v: u64) -> f64 {
    (v as f64 / (1024.0 * 1024.0) * 10.0).round() / 10.0
}

fn gib(v: u64) -> f64 {
    (v as f64 / (1024.0 * 1024.0 * 1024.0) * 100.0).round() / 100.0
}

#[cfg(windows)]
pub fn snapshot() -> Snapshot {
    use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX};
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    unsafe {
        let mut pmc: PROCESS_MEMORY_COUNTERS_EX = std::mem::zeroed();
        pmc.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
        GetProcessMemoryInfo(GetCurrentProcess(), &mut pmc as *mut _ as *mut PROCESS_MEMORY_COUNTERS, pmc.cb);
        let mut ms: MEMORYSTATUSEX = std::mem::zeroed();
        ms.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        GlobalMemoryStatusEx(&mut ms);
        Snapshot { resident: pmc.PrivateUsage as u64, virtual_bytes: ms.ullTotalVirtual - ms.ullAvailVirtual }
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
pub fn snapshot() -> Snapshot {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let field = |name: &str| {
        status
            .lines()
            .find(|l| l.starts_with(name))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse::<u64>().ok())
            .map(|kib| kib * 1024)
            .unwrap_or(0)
    };
    Snapshot { resident: field("VmRSS:"), virtual_bytes: field("VmSize:") }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "android")))]
pub fn snapshot() -> Snapshot {
    Snapshot::default()
}
