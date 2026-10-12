//! `spike-run [sections] [modes]`: comma-separated lists, empty or
//! omitted for all. Sections: synthetic, signing, soundcloud, ten. Modes:
//! cranelift+default-memory, cranelift+small-memory, pulley+default-memory,
//! pulley+small-memory.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let sections = args.first().map(String::as_str).unwrap_or("");
    let modes = args.get(1).map(String::as_str).unwrap_or("");
    for line in plugin_host_spike::run_from_args(sections, modes) {
        println!("{line}");
    }
}
