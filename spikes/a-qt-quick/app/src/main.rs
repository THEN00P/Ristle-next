//! Spike A: Qt Quick desktop shell measured with the frame-timing method.
//! Configuration comes from `SPIKE_*` environment variables (see README).

mod catalog;
mod models;
mod queue;
mod spike;
mod tracks;

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QString, QUrl};

fn env_i32(name: &str, default: i32) -> i32 {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn main() {
    let mut app = QGuiApplication::new();
    let mut engine = QQmlApplicationEngine::new();

    let asset_dir = std::env::var("SPIKE_ASSETS").unwrap_or_else(|_| {
        concat!(env!("CARGO_MANIFEST_DIR"), "/assets").replace(std::path::MAIN_SEPARATOR, "/")
    });
    // 0 = Qt (distance field), 1 = native (DirectWrite glyphs), 2 = curve.
    let text_render_type = env_i32("SPIKE_TEXT", 1);
    let art_threads = env_i32("SPIKE_ART_THREADS", 4);

    if let Some(mut engine) = engine.as_mut() {
        spike::ffi::spike_probe_setup(
            engine.as_mut(),
            art_threads,
            &QString::from(asset_dir.as_str()),
            text_render_type,
        );
        engine.as_mut().load(&QUrl::from("qrc:/qt/qml/Ristle/Spike/qml/Main.qml"));
        spike::ffi::spike_probe_install(engine);
    }

    if let Some(app) = app.as_mut() {
        app.exec();
    }
}
