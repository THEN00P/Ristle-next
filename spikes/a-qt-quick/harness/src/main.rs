//! Out-of-process measurement tools for spike A. Every timestamp is QPC so it
//! lines up with PresentMon and the app's ring buffer.
//!
//!   harness input <script> <log.csv>          scripted SendInput / touch injection
//!   harness dupcap <args>                      per-frame Desktop Duplication capture
//!   harness display get | refresh <hz> | scale <percent>
//!   harness procstat <pid> <ms> <secs> <out>   memory and CPU of a process, plus system CPU
//!   harness window <title>                     client rect (physical px) and DPI
//!   harness qpc                                prints QPC and its frequency

mod display;
mod dupcap;
mod input;
mod procstat;

use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};

pub fn qpc() -> i64 {
    let mut t = 0i64;
    unsafe { QueryPerformanceCounter(&mut t).unwrap() };
    t
}

pub fn qpc_freq() -> i64 {
    let mut f = 0i64;
    unsafe { QueryPerformanceFrequency(&mut f).unwrap() };
    f
}

fn main() {
    // Physical pixels everywhere: input coordinates, capture rects, window rects.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest = &args[1.min(args.len())..];
    let result = match args.first().map(String::as_str) {
        Some("input") => input::run(rest),
        Some("dupcap") => dupcap::run(rest),
        Some("display") => display::run(rest),
        Some("procstat") => procstat::run(rest),
        Some("window") => display::window(rest),
        Some("qpc") => {
            println!("{} {}", qpc(), qpc_freq());
            Ok(())
        }
        _ => Err("usage: harness input|dupcap|display|procstat|window|qpc ...".into()),
    };
    if let Err(e) = result {
        eprintln!("harness: {e}");
        std::process::exit(1);
    }
}

pub type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
