# Spike A: Qt Quick desktop shell and frame-timing harness

Results: `docs/research/qt-quick-spike.md`. This directory is disposable; the
harness (`harness/`, `tools/`) is worth keeping for the GPUI comparison.

## Build

Qt 6.11.3 without an account (aqtinstall from git main reads the 6.11 layout):

    pip install git+https://github.com/miurahr/aqtinstall
    aqt install-qt windows desktop 6.11.3 win64_msvc2022_64 -m qtshadertools qtimageformats -O <dir>
    set QMAKE=<dir>\6.11.3\msvc2022_64\bin\qmake.exe
    cargo build --release

`tools/gen_art.py app/assets/art` generates the 512 artwork JPEGs (not
committed). Run `target/release/qtspike.exe` with Qt's `bin` on `PATH`.
`SPIKE_*` environment variables set the load (see `app/src/spike.rs`); keys:
Ctrl+B sidebar, Ctrl+1/2 pages, Ins/Del/Ctrl+M queue edits, F2 queue churn,
F5–F9 and Shift+F5–F9 injected GUI and render stalls.

## Measure

- `harness.exe`: `input` (scripted SendInput and touch), `dupcap` (Desktop
  Duplication, every frame), `display` (refresh rate and scaling), `procstat`,
  `window`, `place`, `textref`.
- `tools/matrix.py`, `matrix_d3d12.py`, `extension.py`: the full runs, one
  display condition at a time, restoring 59.951 Hz / 100 % afterwards.
- `tools/report_data.py` turns `results/*.jsonl` into `results/tables.md` and
  `results/ramps.md`. `tools/text_analysis.py <tag>` measures text crops.

Raw runs (PresentMon CSVs, ring-buffer dumps, duplication logs) are in
`results/raw/*.7z`, one archive per condition.
