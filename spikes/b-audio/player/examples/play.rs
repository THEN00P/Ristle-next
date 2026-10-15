// Plays the gapless album through the default output for N seconds,
// seeks once, and prints state and stats. `cargo run --example play -- DIR SECONDS`
use ristle_audio::{Player, PlayerState, StateObserver};
struct Print;
impl StateObserver for Print {
    fn on_state(&self, _s: PlayerState) {}
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let dir = &a[1];
    let secs: u64 = a[2].parse().unwrap();
    let paths: Vec<String> = ["t0.mp3", "t1.mp3", "t2.mp3", "t0.m4a", "t1.m4a", "t2.m4a"].iter().map(|f| format!("{dir}/{f}")).collect();
    let titles = paths.iter().map(|p| p.rsplit('/').next().unwrap().to_string()).collect();
    let p = Player::new(paths, titles, true).expect("player");
    p.set_observer(std::sync::Arc::new(Print));
    p.play();
    for i in 0..secs {
        std::thread::sleep(std::time::Duration::from_secs(1));
        if i == 3 { p.seek(1, 9_000); }
        let s = p.state();
        let st = p.stats();
        println!("{:?} track {} {} {}ms/{}ms | starved {} xruns {} cb {} maxcb {}us decodeCpu {}ms cbCpu {}ms transitions {}",
            s.status, s.track_index, s.title, s.position_ms, s.duration_ms, st.starved_callbacks, st.xruns, st.callbacks, st.max_callback_us, st.decode_cpu_ms, st.callback_cpu_ms, st.transitions);
    }
}
