// Feeds init.mp4 + segments from N as one non-seekable stream and counts packets.
use std::io::Cursor;
use symphonia::core::io::ReadOnlySource;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let dir = &a[1];
    let start: usize = a[2].parse().unwrap();
    let seekable = a.get(3).is_some();
    let mut bytes = std::fs::read(format!("{dir}/init.mp4")).unwrap();
    for i in start..start + 3 {
        bytes.extend(std::fs::read(format!("{dir}/seg{i:03}.m4s")).unwrap());
    }
    let src: Box<dyn symphonia::core::io::MediaSource> = if seekable { Box::new(Cursor::new(bytes)) } else { Box::new(ReadOnlySource::new(Cursor::new(bytes))) };
    let mut f = audiolab::decode::open(src, Some("mp4")).unwrap();
    let mut n = 0; let mut first = None; let mut last = 0;
    loop {
        match f.next_packet() {
            Ok(Some(p)) => { n += 1; first.get_or_insert(p.pts.get()); last = p.pts.get(); }
            Ok(None) => { println!("end"); break; }
            Err(e) => { println!("err {e}"); break; }
        }
    }
    println!("packets {n} first_pts {first:?} last_pts {last}");
}
