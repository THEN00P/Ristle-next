use std::{env, fs, path::PathBuf};

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let guest = env::var("RISTLE_SPIKE_GUEST_WASM").map(PathBuf::from).unwrap_or_else(|_| {
        manifest.join("../target/wasm32-unknown-unknown/release/spike_guest.wasm")
    });
    println!("cargo:rerun-if-changed={}", guest.display());
    println!("cargo:rerun-if-env-changed=RISTLE_SPIKE_GUEST_WASM");
    let module = fs::read(&guest).unwrap_or_else(|e| {
        panic!("build the guest first (cargo build -p spike-guest --target wasm32-unknown-unknown --release): {} ({e})", guest.display())
    });
    let component = wit_component::ComponentEncoder::default()
        .module(&module)
        .expect("guest module")
        .validate(true)
        .encode()
        .expect("encode component");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("plugin.component.wasm");
    fs::write(out, component).unwrap();
}
