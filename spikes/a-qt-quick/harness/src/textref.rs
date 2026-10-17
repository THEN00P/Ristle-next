//! Native text reference for the sharpness comparison (scenario 5).
//!
//!   harness textref <secs> cleartype|grayscale|gdi
//!
//! Opens "Ristle text reference" with the same client origin and size as the
//! Qt app (80, 60 / 1280 x 720 logical) and draws the Segoe UI strings of
//! TextSpecimen.qml at the same positions, colours and pixel sizes: dark
//! Lumen band on top, white band below. One API per window:
//! Direct2D/DirectWrite with ClearType or grayscale antialiasing, or GDI with
//! ClearType (what classic Win32 lists such as File Explorer use).

use crate::Result;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

// Same strings, sizes, weights and positions as the first three rows of the
// QML specimen (Column at x 24 + col * 410, y 24, spacing 18, label row first).
const SAMPLES: [(f32, u32, &str); 3] = [
    (12.0, 400, "Floating Points · Crush · 3:51"),
    (13.0, 400, "Ólafur Arnalds · re:member"),
    (14.0, 500, "Midnight Pilot (Edit) — Saturn"),
];
const DARK: (u8, u8, u8) = (0x1b, 0x17, 0x12);
const CREAM: (u8, u8, u8) = (0xf5, 0xf0, 0xe8);

static MODE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0); // 0 cleartype, 1 grayscale, 2 gdi

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// Top of each sample row in logical px, approximating the QML Column: the
/// label row (11 px mono, ~15 px tall) then 18 px spacing between rows.
fn rows(band: usize) -> Vec<f32> {
    let mut y = band as f32 * 360.0 + 24.0 + 15.0 + 18.0;
    let mut out = Vec::new();
    for (size, _, _) in SAMPLES {
        out.push(y);
        y += (size * 1.33).round() + 18.0;
    }
    out
}

fn col_x(col: usize) -> f32 {
    24.0 + col as f32 * 410.0
}

unsafe fn paint_d2d(hwnd: HWND, grayscale: bool) -> Result {
    let dpi = unsafe { GetDpiForWindow(hwnd) } as f32;
    let mut rc = RECT::default();
    unsafe { GetClientRect(hwnd, &mut rc)? };
    let factory: ID2D1Factory = unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)? };
    let dw: IDWriteFactory = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)? };
    let props = D2D1_RENDER_TARGET_PROPERTIES { dpiX: dpi, dpiY: dpi, ..Default::default() };
    let hprops = D2D1_HWND_RENDER_TARGET_PROPERTIES {
        hwnd,
        pixelSize: D2D_SIZE_U { width: rc.right as u32, height: rc.bottom as u32 },
        presentOptions: D2D1_PRESENT_OPTIONS_NONE,
    };
    let target = unsafe { factory.CreateHwndRenderTarget(&props, &hprops)? };
    let c = |(r, g, b): (u8, u8, u8)| D2D1_COLOR_F { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a: 1.0 };
    unsafe {
        target.BeginDraw();
        target.Clear(Some(&c((255, 255, 255))));
        let dark = target.CreateSolidColorBrush(&c(DARK), None)?;
        target.FillRectangle(&D2D_RECT_F { left: 0.0, top: 0.0, right: 1280.0, bottom: 360.0 }, &dark);
        target.SetTextAntialiasMode(if grayscale { D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE } else { D2D1_TEXT_ANTIALIAS_MODE_CLEARTYPE });
        for band in 0..2 {
            let brush = target.CreateSolidColorBrush(&c(if band == 0 { CREAM } else { (0, 0, 0) }), None)?;
            for (i, y) in rows(band).into_iter().enumerate() {
                let (size, weight, text) = SAMPLES[i];
                let format = dw.CreateTextFormat(w!("Segoe UI"), None, DWRITE_FONT_WEIGHT(weight as i32),
                    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size, w!("en-us"))?;
                let t = wide(text);
                for col in 0..3 {
                    let x = col_x(col);
                    let r = D2D_RECT_F { left: x, top: y, right: x + 400.0, bottom: y + 40.0 };
                    target.DrawText(&t, &format, &r, &brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
                }
            }
        }
        target.EndDraw(None, None)?;
    }
    Ok(())
}

unsafe fn paint_gdi(hwnd: HWND) -> Result {
    let k = unsafe { GetDpiForWindow(hwnd) } as f32 / 96.0;
    let mut ps = PAINTSTRUCT::default();
    let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
    let rgb = |(r, g, b): (u8, u8, u8)| COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16);
    unsafe {
        let dark = CreateSolidBrush(rgb(DARK));
        let white = CreateSolidBrush(rgb((255, 255, 255)));
        let mut rc = RECT::default();
        GetClientRect(hwnd, &mut rc)?;
        let band = (360.0 * k).round() as i32;
        FillRect(hdc, &RECT { left: 0, top: 0, right: rc.right, bottom: band }, dark);
        FillRect(hdc, &RECT { left: 0, top: band, right: rc.right, bottom: rc.bottom }, white);
        let _ = DeleteObject(dark.into());
        let _ = DeleteObject(white.into());
        SetBkMode(hdc, TRANSPARENT);
        for b in 0..2 {
            SetTextColor(hdc, rgb(if b == 0 { CREAM } else { (0, 0, 0) }));
            for (i, y) in rows(b).into_iter().enumerate() {
                let (size, weight, text) = SAMPLES[i];
                let font = CreateFontW(-((size * k).round() as i32), 0, 0, 0, weight as i32, 0, 0, 0, DEFAULT_CHARSET,
                    OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, 0, w!("Segoe UI"));
                let old = SelectObject(hdc, font.into());
                for col in 0..3 {
                    let mut t = wide(text);
                    let x = (col_x(col) * k).round() as i32;
                    let top = (y * k).round() as i32;
                    let mut r = RECT { left: x, top, right: x + (400.0 * k) as i32, bottom: top + (40.0 * k) as i32 };
                    DrawTextW(hdc, &mut t, &mut r, DT_LEFT | DT_TOP | DT_SINGLELINE | DT_NOPREFIX);
                }
                SelectObject(hdc, old);
                let _ = DeleteObject(font.into());
            }
        }
        let _ = EndPaint(hwnd, &ps);
    }
    Ok(())
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mode = MODE.load(std::sync::atomic::Ordering::Relaxed);
            let r = if mode == 2 {
                unsafe { paint_gdi(hwnd) }
            } else {
                let r = unsafe { paint_d2d(hwnd, mode == 1) };
                unsafe { let _ = ValidateRect(Some(hwnd), None); };
                r
            };
            if let Err(e) = r {
                eprintln!("textref paint: {e}");
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

pub fn run(args: &[String]) -> Result {
    let secs: u64 = args.first().map(|s| s.parse()).transpose()?.unwrap_or(10);
    let mode = match args.get(1).map(String::as_str).unwrap_or("cleartype") {
        "cleartype" => 0,
        "grayscale" => 1,
        "gdi" => 2,
        m => return Err(format!("unknown mode {m}").into()),
    };
    MODE.store(mode, std::sync::atomic::Ordering::Relaxed);
    let instance = unsafe { GetModuleHandleW(None)? };
    let class = w!("RistleTextRef");
    let wc = WNDCLASSW {
        lpfnWndProc: Some(wndproc),
        hInstance: instance.into(),
        lpszClassName: class,
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW)? },
        ..Default::default()
    };
    unsafe { RegisterClassW(&wc) };
    let k = unsafe { windows::Win32::UI::HiDpi::GetDpiForSystem() } as f32 / 96.0;
    let style = WS_OVERLAPPEDWINDOW;
    let mut r = RECT { left: (80.0 * k) as i32, top: (60.0 * k) as i32, right: (1360.0 * k) as i32, bottom: (780.0 * k) as i32 };
    unsafe { windows::Win32::UI::HiDpi::AdjustWindowRectExForDpi(&mut r, style, false, WINDOW_EX_STYLE(0), (k * 96.0) as u32)? };
    let hwnd = unsafe {
        CreateWindowExW(WINDOW_EX_STYLE(0), class, w!("Ristle text reference"), style | WS_VISIBLE,
                        r.left, r.top, r.right - r.left, r.bottom - r.top, None, None, Some(instance.into()), None)?
    };
    unsafe {
        // Topmost so the capture sees it, and repainted every 100 ms so the
        // duplication recorder keeps getting frames from a static window.
        SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE)?;
        let _ = SetForegroundWindow(hwnd);
        SetTimer(Some(hwnd), 1, (secs * 1000) as u32, None);
        SetTimer(Some(hwnd), 2, 100, None);
    }
    let mut msg = MSG::default();
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
        if msg.message == WM_TIMER && msg.hwnd == hwnd && (msg.wParam.0 == 1 || msg.wParam.0 == 2) {
            if msg.wParam.0 == 2 {
                unsafe { let _ = InvalidateRect(Some(hwnd), None, false); };
            } else {
                unsafe { DestroyWindow(hwnd)? };
            }
            continue;
        }
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}
