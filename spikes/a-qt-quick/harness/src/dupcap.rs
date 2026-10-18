//! Per-frame Desktop Duplication capture.
//!
//!   dupcap --secs <s> --out <prefix> [--rect name,x,y,w,h]... [--marker x,y0,y1]
//!          [--snap <t_ms>]...
//!
//! Every desktop frame DWM produces is acquired. For each one we log its DWM
//! present time (QPC), the number of frames DWM accumulated since the last
//! acquire (anything above 1 means this recorder missed a frame and the run is
//! flagged), a hash per named rect (to see which regions changed), and the
//! decoded queue-marker runs. `--snap` saves the capture box as PNG at the
//! first frame after each given time. Coordinates are physical pixels on the
//! primary output.

use crate::{Result, qpc, qpc_freq};
use std::io::Write;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::core::Interface;

struct Rect {
    name: String,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

struct Frame {
    present_qpc: i64,
    acquire_qpc: i64,
    accumulated: u32,
    hashes: Vec<u64>,
    pixels: Vec<u32>,
    markers: String,
}

fn parse_ints(s: &str) -> Result<Vec<i32>> {
    s.split(',').map(|v| Ok(v.trim().parse::<i32>()?)).collect()
}

fn primary_output() -> Result<(IDXGIAdapter1, IDXGIOutput1)> {
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1()? };
    let mut a = 0;
    while let Ok(adapter) = unsafe { factory.EnumAdapters1(a) } {
        let mut o = 0;
        while let Ok(output) = unsafe { adapter.EnumOutputs(o) } {
            let desc = unsafe { output.GetDesc()? };
            if desc.AttachedToDesktop.as_bool()
                && desc.DesktopCoordinates.left == 0
                && desc.DesktopCoordinates.top == 0
            {
                return Ok((adapter, output.cast()?));
            }
            o += 1;
        }
        a += 1;
    }
    Err("no primary output".into())
}

pub fn run(args: &[String]) -> Result {
    let mut secs = 10.0f64;
    let mut out = String::from("dupcap");
    let mut rects: Vec<Rect> = Vec::new();
    let mut marker: Option<(i32, i32, i32, i32)> = None;
    let mut snaps: Vec<f64> = Vec::new();
    let mut pixels: Vec<(String, i32, i32)> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut v = || it.next().ok_or("missing value");
        match a.as_str() {
            "--secs" => secs = v()?.parse()?,
            "--out" => out = v()?.clone(),
            "--rect" => {
                let s = v()?;
                let (name, nums) = s.split_once(',').ok_or("rect name,x,y,w,h")?;
                let n = parse_ints(nums)?;
                rects.push(Rect { name: name.into(), x: n[0], y: n[1], w: n[2], h: n[3] });
            }
            "--marker" => {
                // x,y0,y1,x_check: the id column and its check column.
                let n = parse_ints(v()?)?;
                marker = Some((n[0], n[1], n[2], n[3]));
            }
            "--snap" => snaps.push(v()?.parse()?),
            "--pixel" => {
                let s = v()?;
                let (name, nums) = s.split_once(',').ok_or("pixel name,x,y")?;
                let n = parse_ints(nums)?;
                pixels.push((name.into(), n[0], n[1]));
            }
            other => return Err(format!("unknown flag {other}").into()),
        }
    }
    if rects.is_empty() {
        return Err("at least one --rect".into());
    }

    // The capture box is the union of every rect and the marker column.
    let mut bx0 = rects.iter().map(|r| r.x).min().unwrap();
    let mut by0 = rects.iter().map(|r| r.y).min().unwrap();
    let mut bx1 = rects.iter().map(|r| r.x + r.w).max().unwrap();
    let mut by1 = rects.iter().map(|r| r.y + r.h).max().unwrap();
    for (_, px, py) in &pixels {
        bx0 = bx0.min(*px);
        bx1 = bx1.max(px + 1);
        by0 = by0.min(*py);
        by1 = by1.max(py + 1);
    }
    if let Some((mx, my0, my1, mx2)) = marker {
        bx0 = bx0.min(mx).min(mx2);
        bx1 = bx1.max(mx + 1).max(mx2 + 1);
        by0 = by0.min(my0);
        by1 = by1.max(my1);
    }
    let (bw, bh) = ((bx1 - bx0) as u32, (by1 - by0) as u32);

    // A corruption check needs every frame, so the recorder outranks the app
    // (duplication passes are never used for timing).
    unsafe {
        use windows::Win32::System::Threading::*;
        let _ = SetPriorityClass(GetCurrentProcess(), HIGH_PRIORITY_CLASS);
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL);
    }
    let (adapter, output) = primary_output()?;
    let mut device = None;
    let mut context = None;
    unsafe {
        D3D11CreateDevice(
            &adapter,
            D3D_DRIVER_TYPE_UNKNOWN,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )?;
    }
    let device: ID3D11Device = device.ok_or("no device")?;
    let context: ID3D11DeviceContext = context.ok_or("no context")?;
    let dup = unsafe { output.DuplicateOutput(&device)? };
    let dup_desc = unsafe { dup.GetDesc() };
    if dup_desc.ModeDesc.Format != DXGI_FORMAT_B8G8R8A8_UNORM {
        return Err(format!("unexpected desktop format {:?}", dup_desc.ModeDesc.Format).into());
    }

    let staging_desc = D3D11_TEXTURE2D_DESC {
        Width: bw,
        Height: bh,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        MiscFlags: 0,
    };
    let mut staging = None;
    unsafe { device.CreateTexture2D(&staging_desc, None, Some(&mut staging))? };
    let staging: ID3D11Texture2D = staging.ok_or("no staging texture")?;
    let src_box = D3D11_BOX {
        left: bx0 as u32,
        top: by0 as u32,
        front: 0,
        right: bx1 as u32,
        bottom: by1 as u32,
        back: 1,
    };

    let freq = qpc_freq();
    let mut frames: Vec<Frame> = Vec::with_capacity((secs * 260.0) as usize);
    let mut snap_images: Vec<(f64, i64, Vec<u8>)> = Vec::new();
    snaps.sort_by(f64::total_cmp);
    let mut next_snap = 0;
    let start = qpc();
    let end = start + (secs * freq as f64) as i64;
    let mut timeouts = 0u32;

    while qpc() < end {
        let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut resource: Option<IDXGIResource> = None;
        match unsafe { dup.AcquireNextFrame(50, &mut info, &mut resource) } {
            Ok(()) => {}
            Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => {
                timeouts += 1;
                continue;
            }
            Err(e) => return Err(format!("AcquireNextFrame: {e}").into()),
        }
        let acquire_qpc = qpc();
        if info.LastPresentTime == 0 {
            // Pointer-only update, no new desktop image.
            unsafe { dup.ReleaseFrame()? };
            continue;
        }
        let texture: ID3D11Texture2D = resource.ok_or("no resource")?.cast()?;
        unsafe {
            context.CopySubresourceRegion(&staging, 0, 0, 0, 0, &texture, 0, Some(&src_box));
        }
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe { context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))? };
        let pitch = mapped.RowPitch as usize;
        let data = unsafe { std::slice::from_raw_parts(mapped.pData as *const u8, pitch * bh as usize) };

        let hashes = rects
            .iter()
            .map(|r| {
                let mut h: u64 = 0xcbf2_9ce4_8422_2325;
                for y in (r.y - by0)..(r.y - by0 + r.h) {
                    let row = &data[y as usize * pitch + (r.x - bx0) as usize * 4..][..r.w as usize * 4];
                    for chunk in row.chunks_exact(8) {
                        h ^= u64::from_le_bytes(chunk.try_into().unwrap());
                        h = h.wrapping_mul(0x0100_0000_01b3);
                    }
                }
                h
            })
            .collect();

        let pixel_values = pixels
            .iter()
            .map(|(_, px, py)| {
                let p = &data[(py - by0) as usize * pitch + (px - bx0) as usize * 4..][..4];
                ((p[2] as u32) << 16) | ((p[1] as u32) << 8) | p[0] as u32
            })
            .collect();

        let mut markers = String::new();
        if let Some((mx, my0, my1, mx2)) = marker {
            let mut run_start = 0i32;
            let mut run_id = 0u32;
            for y in my0..=my1 {
                let id = if y < my1 {
                    let row = (y - by0) as usize * pitch;
                    let p = &data[row + (mx - bx0) as usize * 4..][..4];
                    let c = &data[row + (mx2 - bx0) as usize * 4..][..4];
                    let (b, g, r) = (p[0] as u32, p[1] as u32, p[2] as u32);
                    let check = [
                        (r * 11 + g * 3 + 0xa5) & 0xff,
                        (r ^ 0x6c).wrapping_add(g * 29) & 0xff,
                        (r * 5 + g * 17 + 0x33) & 0xff,
                    ];
                    let ok = (r * 7 + g * 13 + 0x5a) & 0xff == b
                        && c[2] as u32 == check[0]
                        && c[1] as u32 == check[1]
                        && c[0] as u32 == check[2];
                    if ok { r | (g << 8) } else { 0 }
                } else {
                    0
                };
                if id != run_id {
                    if run_id != 0 {
                        markers.push_str(&format!("{}-{}:{} ", run_start, y - 1, run_id));
                    }
                    run_id = id;
                    run_start = y;
                }
            }
        }

        if next_snap < snaps.len() {
            let t_ms = (acquire_qpc - start) as f64 * 1000.0 / freq as f64;
            if t_ms >= snaps[next_snap] {
                let mut img = Vec::with_capacity((bw * bh * 4) as usize);
                for y in 0..bh as usize {
                    for px in data[y * pitch..][..bw as usize * 4].chunks_exact(4) {
                        img.extend_from_slice(&[px[2], px[1], px[0], 255]);
                    }
                }
                snap_images.push((snaps[next_snap], info.LastPresentTime, img));
                next_snap += 1;
            }
        }

        unsafe {
            context.Unmap(&staging, 0);
            dup.ReleaseFrame()?;
        }
        frames.push(Frame {
            present_qpc: info.LastPresentTime,
            acquire_qpc,
            accumulated: info.AccumulatedFrames,
            hashes,
            pixels: pixel_values,
            markers,
        });
    }

    let mut f = std::io::BufWriter::new(std::fs::File::create(format!("{out}.csv"))?);
    writeln!(
        f,
        "# qpc_freq={freq} box={bx0},{by0},{bw},{bh} refresh={}/{} timeouts={timeouts}",
        dup_desc.ModeDesc.RefreshRate.Numerator, dup_desc.ModeDesc.RefreshRate.Denominator
    )?;
    let names: Vec<&str> = rects.iter().map(|r| r.name.as_str()).collect();
    let pixel_names: Vec<String> = pixels.iter().map(|p| format!("px_{}", p.0)).collect();
    let mut header = names.join(",");
    for n in &pixel_names {
        header.push(',');
        header.push_str(n);
    }
    writeln!(f, "frame,present_qpc,acquire_qpc,accumulated,{header},markers")?;
    for (i, fr) in frames.iter().enumerate() {
        let mut hs: Vec<String> = fr.hashes.iter().map(|h| format!("{h:016x}")).collect();
        hs.extend(fr.pixels.iter().map(|p| format!("{p:06x}")));
        writeln!(
            f,
            "{i},{},{},{},{},{}",
            fr.present_qpc,
            fr.acquire_qpc,
            fr.accumulated,
            hs.join(","),
            fr.markers.trim_end()
        )?;
    }
    for (t, present, img) in snap_images {
        let file = std::fs::File::create(format!("{out}_snap{}.png", t as i64))?;
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), bw, bh);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header()?;
        w.write_image_data(&img)?;
        eprintln!("snap at {t} ms, present qpc {present}");
    }
    eprintln!("dupcap: {} frames, {} timeouts", frames.len(), timeouts);
    Ok(())
}
