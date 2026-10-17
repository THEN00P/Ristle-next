//! Display mode and scaling for the primary display, and window geometry.
//!
//! Scaling uses the DPI-scale device-info packets that the Settings app uses
//! (types -3 and -4, undocumented). The value is relative to the recommended
//! scale, so `scale` converts a percentage to that step.

use crate::Result;
use windows::Win32::Devices::Display::*;
use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::*;

const SCALES: [u32; 12] = [100, 125, 150, 175, 200, 225, 250, 300, 350, 400, 450, 500];

#[repr(C)]
struct DpiGet {
    header: DISPLAYCONFIG_DEVICE_INFO_HEADER,
    min_rel: i32,
    cur_rel: i32,
    max_rel: i32,
}

#[repr(C)]
struct DpiSet {
    header: DISPLAYCONFIG_DEVICE_INFO_HEADER,
    rel: i32,
}

fn primary_path() -> Result<DISPLAYCONFIG_PATH_INFO> {
    let (mut np, mut nm) = (0u32, 0u32);
    unsafe { GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut np, &mut nm).ok()? };
    let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); np as usize];
    let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); nm as usize];
    unsafe {
        QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS, &mut np, paths.as_mut_ptr(), &mut nm, modes.as_mut_ptr(), None)
            .ok()?
    };
    paths.truncate(np as usize);
    // The primary display is the source whose mode sits at the desktop origin.
    for p in &paths {
        let idx = unsafe { p.sourceInfo.Anonymous.modeInfoIdx } as usize;
        if let Some(m) = modes.get(idx) {
            if m.infoType == DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE {
                let src = unsafe { m.Anonymous.sourceMode };
                if src.position.x == 0 && src.position.y == 0 {
                    return Ok(*p);
                }
            }
        }
    }
    paths.first().copied().ok_or_else(|| "no display path".into())
}

fn scale_info() -> Result<(u32, u32, DISPLAYCONFIG_PATH_INFO)> {
    let path = primary_path()?;
    let mut get = DpiGet {
        header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
            r#type: DISPLAYCONFIG_DEVICE_INFO_TYPE(-3),
            size: size_of::<DpiGet>() as u32,
            adapterId: path.sourceInfo.adapterId,
            id: path.sourceInfo.id,
        },
        min_rel: 0,
        cur_rel: 0,
        max_rel: 0,
    };
    let rc = unsafe { DisplayConfigGetDeviceInfo(&mut get.header) };
    if rc != 0 {
        return Err(format!("DisplayConfigGetDeviceInfo {rc}").into());
    }
    let recommended = get.min_rel.unsigned_abs() as usize;
    let current = (recommended as i32 + get.cur_rel) as usize;
    Ok((SCALES[current.min(11)], SCALES[recommended.min(11)], path))
}

fn current_mode() -> Result<DEVMODEW> {
    let mut dm = DEVMODEW { dmSize: size_of::<DEVMODEW>() as u16, ..Default::default() };
    unsafe { EnumDisplaySettingsW(None, ENUM_CURRENT_SETTINGS, &mut dm).ok()? };
    Ok(dm)
}

pub fn run(args: &[String]) -> Result {
    match args.first().map(String::as_str) {
        Some("get") => {
            let dm = current_mode()?;
            let (scale, recommended, _) = scale_info()?;
            let mut max_hz = 0;
            let mut i = 0;
            loop {
                let mut m = DEVMODEW { dmSize: size_of::<DEVMODEW>() as u16, ..Default::default() };
                if !unsafe { EnumDisplaySettingsW(None, ENUM_DISPLAY_SETTINGS_MODE(i), &mut m) }.as_bool() {
                    break;
                }
                if m.dmPelsWidth == dm.dmPelsWidth && m.dmPelsHeight == dm.dmPelsHeight {
                    max_hz = max_hz.max(m.dmDisplayFrequency);
                }
                i += 1;
            }
            println!(
                "width={} height={} refresh={} max_refresh={} scale={} recommended_scale={}",
                dm.dmPelsWidth, dm.dmPelsHeight, dm.dmDisplayFrequency, max_hz, scale, recommended
            );
            Ok(())
        }
        Some("refresh") => {
            let hz: u32 = args.get(1).ok_or("refresh <hz>")?.parse()?;
            let mut dm = current_mode()?;
            dm.dmDisplayFrequency = hz;
            dm.dmFields = DM_DISPLAYFREQUENCY;
            let r = unsafe { ChangeDisplaySettingsExW(None, Some(&dm), None, CDS_UPDATEREGISTRY, None) };
            if r != DISP_CHANGE_SUCCESSFUL {
                return Err(format!("ChangeDisplaySettingsEx {r:?}").into());
            }
            println!("refresh={}", current_mode()?.dmDisplayFrequency);
            Ok(())
        }
        Some("scale") => {
            let want: u32 = args.get(1).ok_or("scale <percent>")?.parse()?;
            let (_, recommended, path) = scale_info()?;
            let want_idx = SCALES.iter().position(|&s| s == want).ok_or("unsupported scale")? as i32;
            let rec_idx = SCALES.iter().position(|&s| s == recommended).unwrap() as i32;
            let set = DpiSet {
                header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                    r#type: DISPLAYCONFIG_DEVICE_INFO_TYPE(-4),
                    size: size_of::<DpiSet>() as u32,
                    adapterId: path.sourceInfo.adapterId,
                    id: path.sourceInfo.id,
                },
                rel: want_idx - rec_idx,
            };
            let rc = unsafe { DisplayConfigSetDeviceInfo(&set.header) };
            if rc != 0 {
                return Err(format!("DisplayConfigSetDeviceInfo {rc}").into());
            }
            println!("scale={}", scale_info()?.0);
            Ok(())
        }
        _ => Err("display get | refresh <hz> | scale <percent>".into()),
    }
}

/// `place <x> <y> <w> <h> <title...>`: moves and sizes a top-level window so
/// its client area is at the given physical rect.
pub fn place(args: &[String]) -> Result {
    let n: Vec<i32> = args.iter().take(4).map(|v| v.parse()).collect::<std::result::Result<_, _>>()?;
    let title = args[4..].join(" ");
    let wide: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
    let hwnd = unsafe { FindWindowW(None, windows::core::PCWSTR(wide.as_ptr()))? };
    unsafe {
        let _ = ShowWindow(hwnd, SW_RESTORE);
        let mut wr = RECT::default();
        GetWindowRect(hwnd, &mut wr)?;
        let mut cr = RECT::default();
        GetClientRect(hwnd, &mut cr)?;
        let mut origin = POINT { x: 0, y: 0 };
        let _ = ClientToScreen(hwnd, &mut origin);
        // Frame thickness around the client area.
        let (l, t) = (origin.x - wr.left, origin.y - wr.top);
        let (r, b) = (wr.right - origin.x - cr.right, wr.bottom - origin.y - cr.bottom);
        SetWindowPos(hwnd, None, n[0] - l, n[1] - t, n[2] + l + r, n[3] + t + b, SWP_NOZORDER)?;
    }
    Ok(())
}

pub fn window(args: &[String]) -> Result {
    let title = args.join(" ");
    let wide: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
    let hwnd = unsafe { FindWindowW(None, windows::core::PCWSTR(wide.as_ptr()))? };
    let mut rc = RECT::default();
    unsafe { GetClientRect(hwnd, &mut rc)? };
    let mut origin = POINT { x: 0, y: 0 };
    unsafe {
        let _ = ClientToScreen(hwnd, &mut origin);
    }
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    println!(
        "x={} y={} w={} h={} dpi={} pid={} hwnd={:?}",
        origin.x, origin.y, rc.right, rc.bottom, dpi, pid, hwnd.0
    );
    Ok(())
}
