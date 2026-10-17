//! Scripted input. One event per line: `<t_ms> <command> [args]`.
//!
//!   key <vk> down|up          virtual-key code (decimal)
//!   wheel <delta>             WHEEL_DELTA is 120; smaller deltas model a touchpad
//!   move <x> <y>              absolute, physical screen pixels
//!   ldown | lup               left button
//!   touch down|move|up <x> <y>
//!   focus <title>             brings the window to the foreground
//!
//! Each event is sent at its target time (sleep, then spin for the last 2 ms)
//! and its QPC time after SendInput returns is logged.

use crate::{Result, qpc, qpc_freq};
use std::io::Write;
use windows::Win32::Foundation::POINT;
use windows::Win32::Media::{timeBeginPeriod, timeEndPeriod};
use windows::Win32::UI::Controls::POINTER_FEEDBACK_NONE;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Input::Pointer::*;
use windows::Win32::UI::WindowsAndMessaging::*;

fn send(inputs: &[INPUT]) -> Result {
    let n = unsafe { SendInput(inputs, size_of::<INPUT>() as i32) };
    if n as usize != inputs.len() {
        return Err(format!("SendInput sent {n} of {}", inputs.len()).into());
    }
    Ok(())
}

fn mouse(flags: MOUSE_EVENT_FLAGS, dx: i32, dy: i32, data: i32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT { dx, dy, mouseData: data as u32, dwFlags: flags, time: 0, dwExtraInfo: 0 },
        },
    }
}

fn key(vk: u16, up: bool) -> INPUT {
    let mut flags = KEYBD_EVENT_FLAGS(0);
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    // Insert, Delete, arrows etc. are extended keys.
    if matches!(vk, 0x21..=0x28 | 0x2D | 0x2E) {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: VIRTUAL_KEY(vk), wScan: 0, dwFlags: flags, time: 0, dwExtraInfo: 0 },
        },
    }
}

fn absolute(x: i32, y: i32) -> (i32, i32) {
    let (vx, vy, vw, vh) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    };
    (((x - vx) * 65535 + vw / 2) / (vw - 1), ((y - vy) * 65535 + vh / 2) / (vh - 1))
}

fn touch(phase: &str, x: i32, y: i32) -> Result {
    let flags = match phase {
        "down" => POINTER_FLAG_DOWN | POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT,
        "move" => POINTER_FLAG_UPDATE | POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT,
        "up" => POINTER_FLAG_UP,
        _ => return Err(format!("touch phase {phase}").into()),
    };
    let mut info = POINTER_TOUCH_INFO::default();
    info.pointerInfo.pointerType = windows::Win32::UI::WindowsAndMessaging::PT_TOUCH;
    info.pointerInfo.pointerId = 0;
    info.pointerInfo.ptPixelLocation = POINT { x, y };
    info.pointerInfo.pointerFlags = flags;
    info.touchFlags = 0;
    info.touchMask = TOUCH_MASK_CONTACTAREA | TOUCH_MASK_ORIENTATION | TOUCH_MASK_PRESSURE;
    info.rcContact.left = x - 2;
    info.rcContact.right = x + 2;
    info.rcContact.top = y - 2;
    info.rcContact.bottom = y + 2;
    info.orientation = 90;
    info.pressure = 32000;
    unsafe { InjectTouchInput(&[info])? };
    Ok(())
}

pub fn focus(title: &str) -> Result {
    let wide: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
    let hwnd = unsafe { FindWindowW(None, windows::core::PCWSTR(wide.as_ptr()))? };
    unsafe {
        // A foreground change is only allowed after input; an Alt tap unlocks it.
        send(&[key(0x12, false), key(0x12, true)])?;
        let _ = ShowWindow(hwnd, SW_RESTORE);
        let _ = SetForegroundWindow(hwnd);
    }
    Ok(())
}

pub fn run(args: &[String]) -> Result {
    let [script, log] = args else {
        return Err("input <script> <log.csv>".into());
    };
    let text = std::fs::read_to_string(script)?;
    let mut events = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let mut parts = line.split_whitespace();
        let t: f64 = parts.next().unwrap().parse()?;
        let rest: Vec<String> = parts.map(str::to_owned).collect();
        events.push((t, rest));
    }
    let mut touch_ready = false;
    if events.iter().any(|(_, e)| e[0] == "touch") {
        unsafe { InitializeTouchInjection(1, TOUCH_FEEDBACK_NONE)? };
        touch_ready = true;
    }
    let _ = POINTER_FEEDBACK_NONE;

    let freq = qpc_freq() as f64;
    let mut out = std::io::BufWriter::new(std::fs::File::create(log)?);
    writeln!(out, "# qpc_freq={}", freq as i64)?;
    writeln!(out, "i,t_ms,qpc,command")?;
    unsafe { timeBeginPeriod(1) };
    let start = qpc();
    let mut log_rows = Vec::with_capacity(events.len());
    for (i, (t, e)) in events.iter().enumerate() {
        let target = start + (t * freq / 1000.0) as i64;
        loop {
            let remaining_ms = (target - qpc()) as f64 * 1000.0 / freq;
            if remaining_ms <= 0.0 {
                break;
            }
            if remaining_ms > 2.5 {
                std::thread::sleep(std::time::Duration::from_micros(((remaining_ms - 2.0) * 1000.0) as u64));
            } else {
                std::hint::spin_loop();
            }
        }
        let arg = |k: usize| -> Result<i32> { Ok(e.get(k).ok_or("missing argument")?.parse()?) };
        match e[0].as_str() {
            "key" => send(&[key(arg(1)? as u16, e.get(2).map(String::as_str) == Some("up"))])?,
            "wheel" => send(&[mouse(MOUSEEVENTF_WHEEL, 0, 0, arg(1)?)])?,
            "move" => {
                let (x, y) = absolute(arg(1)?, arg(2)?);
                send(&[mouse(MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK, x, y, 0)])?
            }
            "ldown" => send(&[mouse(MOUSEEVENTF_LEFTDOWN, 0, 0, 0)])?,
            "lup" => send(&[mouse(MOUSEEVENTF_LEFTUP, 0, 0, 0)])?,
            "touch" if touch_ready => touch(&e[1], arg(2)?, arg(3)?)?,
            "focus" => focus(&e[1..].join(" "))?,
            other => return Err(format!("unknown command {other}").into()),
        }
        log_rows.push((i, *t, qpc(), e.join(" ")));
    }
    unsafe { timeEndPeriod(1) };
    for (i, t, q, c) in log_rows {
        writeln!(out, "{i},{t},{q},{c}")?;
    }
    Ok(())
}
