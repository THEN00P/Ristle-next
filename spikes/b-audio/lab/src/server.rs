//! Local static HTTP server for HLS fixtures, with simulated network: every
//! response waits `rtt` and is paced to `kbps`.

use std::path::PathBuf;
use std::time::Duration;

pub fn spawn(root: PathBuf, rtt: Duration, kbps: u64) -> String {
    let server = tiny_http::Server::http("127.0.0.1:0").expect("bind");
    let addr = format!("http://{}", server.server_addr().to_ip().unwrap());
    std::thread::spawn(move || {
        for req in server.incoming_requests() {
            let root = root.clone();
            std::thread::spawn(move || {
                let path = root.join(req.url().trim_start_matches('/').split('?').next().unwrap());
                std::thread::sleep(rtt);
                match std::fs::read(&path) {
                    Ok(body) => {
                        let len = body.len();
                        let paced = Paced { data: body, pos: 0, kbps };
                        let _ = req.respond(tiny_http::Response::new(200.into(), vec![], paced, Some(len), None));
                    }
                    Err(_) => {
                        let _ = req.respond(tiny_http::Response::empty(404));
                    }
                }
            });
        }
    });
    addr
}

/// Sends the body in 4 KB slices at `kbps`, so a client can start on the
/// first bytes before the rest arrives.
struct Paced {
    data: Vec<u8>,
    pos: usize,
    kbps: u64,
}

impl std::io::Read for Paced {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = buf.len().min(4096).min(self.data.len() - self.pos);
        if n > 0 && self.kbps > 0 {
            std::thread::sleep(Duration::from_secs_f64(n as f64 * 8.0 / (self.kbps as f64 * 1000.0)));
        }
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}
