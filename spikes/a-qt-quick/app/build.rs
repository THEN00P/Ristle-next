use cxx_qt_build::{CxxQtBuilder, QmlFile, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(QmlModule::new("Ristle.Spike").qml_files([
        QmlFile::from("qml/Main.qml"),
        QmlFile::from("qml/Lumen.qml").singleton(true),
        QmlFile::from("qml/Sidebar.qml"),
        QmlFile::from("qml/TrackTable.qml"),
        QmlFile::from("qml/TrackRow.qml"),
        QmlFile::from("qml/LibraryPage.qml"),
        QmlFile::from("qml/QueuePanel.qml"),
        QmlFile::from("qml/QueueRow.qml"),
        QmlFile::from("qml/NowPlayingBar.qml"),
        QmlFile::from("qml/Artwork.qml"),
        QmlFile::from("qml/TextSpecimen.qml"),
    ]))
    .qt_module("Quick")
    .qt_module("Network")
    .files(["src/spike.rs", "src/models.rs"])
    .cpp_file("cpp/probe.cpp")
    .build();
}
