"""Baselines with the same harness (method rule 6): File Explorer and Ristle
legacy, against the Qt app, on the same scripted flings and wheel.

The comparison metric is the duplication pass's content cadence on the list
region (skipped refreshes while the list is moving), because composition-based
apps do not present through their own swap chain. A PresentMon pass on dwm.exe
(by pid; its name is hidden without elevation) records composed frames.
Usage: baselines.py <tag> qt|explorer|legacy"""
import json, pathlib, subprocess, sys, time
import analyze
from scenarios import SUMMARY, scroll_script, window
from spikelib import HARNESS, TOOLS, display, run, sh, harness

FOLDER = TOOLS / "explorer-baseline"
LIST = (248, 200, 680, 480)          # logical client rect measured in every app
INPUTS = [("fling", 2000), ("fling", 8000), ("wheel", 10)]


def dwm_pid():
    out = sh(["tasklist", "/fi", "imagename eq dwm.exe", "/fo", "csv", "/nh"]).stdout
    return int(out.split(",")[1].strip('"'))


def explorer_launch():
    FOLDER.mkdir(exist_ok=True)
    if len(list(FOLDER.iterdir())) < 3000:
        for i in range(3000):
            (FOLDER / f"track {i:05d} - midnight pilot.txt").write_text("x")
    title = FOLDER.name
    if sh([HARNESS, "window", title]).returncode != 0:  # reuse an open window
        subprocess.Popen(["explorer.exe", str(FOLDER)])
    analyze_wait(title)
    k = int(display()["scale"]) / 100
    harness("place", round(80 * k), round(60 * k), round(1280 * k), round(800 * k), title)
    time.sleep(1.0)
    return None, title


def legacy_launch():
    if sh([HARNESS, "window", "Ristle"]).returncode != 0:
        subprocess.Popen(["explorer.exe", r"shell:AppsFolder\Ristle_3a9eps4eb1xxc!App"])
    analyze_wait("Ristle", 40)
    k = int(display()["scale"]) / 100
    harness("place", round(80 * k), round(60 * k), round(1280 * k), round(800 * k), "Ristle")
    time.sleep(2.0)
    return None, "Ristle"


def analyze_wait(title, timeout=20):
    t0 = time.time()
    while time.time() - t0 < timeout:
        if sh([HARNESS, "window", title]).returncode == 0:
            return
        time.sleep(0.2)
    raise RuntimeError(f"{title} did not open")


def measure_app(tag, app):
    launch = {"qt": None, "explorer": explorer_launch, "legacy": legacy_launch}[app]
    cfg = {"SPIKE_ROWS": 10000, "SPIKE_WEIGHT": 1} if app == "qt" else {}
    for kind, speed in INPUTS:
        name = f"{tag}_baseline_{app}_{kind}{speed}"
        d = run(name + "_dd", cfg, scroll_script(kind, speed), mode="dd", launch=launch,
                rects={"list": LIST}, clocks=(app == "qt"), close=(app == "qt"))
        t0, t1 = window(d)
        cad = analyze.content_cadence(d, "list", t0, t1)
        p = run(name + "_pm", cfg, scroll_script(kind, speed), mode="pm", launch=launch,
                pm_target=dwm_pid() if app != "qt" else None, close=(app == "qt"))
        t0, t1 = window(p)
        hz = analyze.refresh_hz(analyze.meta(p))
        fs = analyze.frame_stats(analyze.presentmon(p), t0, t1, hz)
        res = {"tag": tag, "app": app, "input": f"{kind}:{speed}", "cadence": cad,
               "presentmon_target": "app" if app == "qt" else "dwm", "presentmon": fs}
        with open(SUMMARY.parent / "baselines.jsonl", "a") as f:
            print(json.dumps(res), file=f)
        print(app, kind, speed, cad, {k: fs[k] for k in ("frames", "expected_frames", "p99_ms", "dropped")}, flush=True)
        if app != "qt":
            title = {"explorer": FOLDER.name, "legacy": "Ristle"}[app]
            info = sh([HARNESS, "window", title]).stdout
            if "pid=" in info:
                sh(["taskkill", "/PID", info.split("pid=")[1].split()[0]]) if app == "legacy" else None


if __name__ == "__main__":
    tag = sys.argv[1]
    for app in sys.argv[2:]:
        measure_app(tag, app)
