use std::{env, fs, path::PathBuf};

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let guest = manifest.join("../target/wasm32-unknown-unknown/release/sc_plugin.wasm");
    println!("cargo:rerun-if-changed={}", guest.display());
    let module = fs::read(&guest).unwrap_or_else(|e| {
        panic!("build the Plugin first (cargo build -p sc-plugin --target wasm32-unknown-unknown --release): {} ({e})", guest.display())
    });
    let component = wit_component::ComponentEncoder::default()
        .module(&module)
        .expect("plugin module")
        .validate(true)
        .encode()
        .expect("encode component");
    fs::write(PathBuf::from(env::var("OUT_DIR").unwrap()).join("sc.component.wasm"), component).unwrap();
}
