//! Samples a process's memory and CPU, and whole-system CPU, at a fixed
//! interval. Used for idle memory, the soak, and to record background noise.

use crate::{Result, qpc};
use std::io::Write;
use windows::Win32::Foundation::{CloseHandle, FILETIME};
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX};
use windows::Win32::System::Threading::*;

fn ft(f: FILETIME) -> u64 {
    ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64
}

pub fn run(args: &[String]) -> Result {
    let [pid, interval, secs, out] = args else {
        return Err("procstat <pid|0> <interval_ms> <secs> <out.csv>".into());
    };
    let pid: u32 = pid.parse()?;
    let interval: u64 = interval.parse()?;
    let secs: f64 = secs.parse()?;
    let process = if pid != 0 {
        Some(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, false, pid)? })
    } else {
        None
    };
    let mut f = std::io::BufWriter::new(std::fs::File::create(out)?);
    writeln!(f, "qpc,private_mb,working_set_mb,proc_cpu_pct,system_busy_pct")?;
    let cpus = std::thread::available_parallelism()?.get() as f64;
    let (mut last_idle, mut last_kernel, mut last_user) = (0u64, 0u64, 0u64);
    let mut last_proc = 0u64;
    let start = std::time::Instant::now();
    let mut next = start;
    let mut first = true;
    while start.elapsed().as_secs_f64() < secs {
        let (mut idle, mut kernel, mut user) = Default::default();
        unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user))? };
        let (idle, kernel, user) = (ft(idle), ft(kernel), ft(user));
        let total = (kernel - last_kernel) + (user - last_user);
        let busy = if total > 0 { 100.0 * (total - (idle - last_idle)) as f64 / total as f64 } else { 0.0 };
        let (mut private, mut ws, mut proc_pct) = (0.0, 0.0, 0.0);
        if let Some(h) = process {
            let mut pmc = PROCESS_MEMORY_COUNTERS_EX::default();
            let ok = unsafe {
                GetProcessMemoryInfo(h, &mut pmc as *mut _ as *mut _, size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32)
            };
            if ok.is_err() {
                break; // process exited
            }
            private = pmc.PrivateUsage as f64 / 1048576.0;
            ws = pmc.WorkingSetSize as f64 / 1048576.0;
            let (mut c, mut e, mut k, mut u) = Default::default();
            unsafe { GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u)? };
            let p = ft(k) + ft(u);
            // FILETIME is 100 ns; the system totals already sum over all CPUs.
            let wall = interval as f64 * 10_000.0;
            proc_pct = if first { 0.0 } else { 100.0 * (p - last_proc) as f64 / (wall * cpus) };
            last_proc = p;
        }
        if !first {
            writeln!(f, "{},{private:.1},{ws:.1},{proc_pct:.2},{busy:.2}", qpc())?;
        }
        first = false;
        (last_idle, last_kernel, last_user) = (idle, kernel, user);
        next += std::time::Duration::from_millis(interval);
        if let Some(d) = next.checked_duration_since(std::time::Instant::now()) {
            std::thread::sleep(d);
        }
    }
    if let Some(h) = process {
        unsafe { CloseHandle(h)? };
    }
    Ok(())
}
