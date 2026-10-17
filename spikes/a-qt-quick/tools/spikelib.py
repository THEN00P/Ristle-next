"""Orchestration for spike A timing runs (docs/research/frame-timing-method.md).

One run = one app launch with a fixed configuration, recorded by PresentMon
(primary evidence), Desktop Duplication (every desktop frame), a process
sampler, and the app's ring buffer (attribution only). Input is a scripted
SendInput/touch sequence executed by `harness input`.
"""
from __future__ import annotations

import csv, json, os, pathlib, subprocess, time

ROOT = pathlib.Path(__file__).resolve().parents[1]
TOOLS = pathlib.Path(r"C:\Users\loren\spike-a-tools")
QT_BIN = TOOLS / r"qt\6.11.3\msvc2022_64\bin"
PRESENTMON = TOOLS / r"presentmon\PresentMon-2.6.0-x64.exe"
HARNESS = ROOT / r"target\release\harness.exe"
APP = ROOT / r"target\release\qtspike.exe"
RUNS = ROOT / "runs"
TITLE = "Ristle spike A"

# Processes that must not run during timing (CLAUDE.md: emulator, Gradle, builds).
HEAVY = ["qemu-system", "emulator", "java", "gradle", "cargo", "rustc", "cl.exe",
         "link.exe", "msbuild", "studio64"]

VK = {"F2": 0x71, "F5": 0x74, "F6": 0x75, "F7": 0x76, "F8": 0x77, "F9": 0x78,
      "SHIFT": 0x10, "CTRL": 0x11, "B": 0x42, "M": 0x4D, "1": 0x31, "2": 0x32,
      "INS": 0x2D, "DEL": 0x2E}


def sh(args, **kw):
    return subprocess.run([str(a) for a in args], capture_output=True, text=True, **kw)


def harness(*args):
    r = sh([HARNESS, *args])
    if r.returncode != 0:
        raise RuntimeError(f"harness {args}: {r.stderr}")
    return r.stdout.strip()


def kv(line):
    return dict(p.split("=", 1) for p in line.split())


def display():
    return kv(harness("display", "get"))


def power_plan():
    return sh(["powercfg", "/getactivescheme"]).stdout.strip()


def heavy_processes():
    out = sh(["tasklist", "/fo", "csv", "/nh"]).stdout.lower()
    names = {row[0] for row in csv.reader(out.splitlines()) if row}
    return sorted(n for n in names if any(h in n for h in HEAVY))


def background_cpu(secs=2.0):
    f = RUNS / "_bg.csv"
    harness("procstat", "0", "250", str(secs), f)
    rows = list(csv.DictReader(open(f)))
    return round(sum(float(r["system_busy_pct"]) for r in rows) / max(1, len(rows)), 2)


def free_memory_mb():
    r = sh(["powershell", "-NoProfile", "-Command",
            "(Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory"])
    return int(r.stdout.strip()) // 1024


class Script:
    """Builds a `harness input` script. Times are milliseconds from start."""

    def __init__(self):
        self.lines = []

    def add(self, t, *cmd):
        self.lines.append((float(t), " ".join(str(c) for c in cmd)))

    def key(self, t, name, hold=30, shift=False, ctrl=False):
        mods = (["SHIFT"] if shift else []) + (["CTRL"] if ctrl else [])
        for m in mods:
            self.add(t, "key", VK[m], "down")
        self.add(t + 1, "key", VK[name], "down")
        self.add(t + hold, "key", VK[name], "up")
        for m in mods:
            self.add(t + hold + 1, "key", VK[m], "up")

    def touch_drag(self, t, x, y0, y1, dur_ms, hz=240, release=True):
        """Finger drag at constant velocity; on release the list flings."""
        self.add(t, "touch", "down", x, y0)
        n = max(1, int(dur_ms * hz / 1000))
        for i in range(1, n + 1):
            self.add(t + i * 1000 / hz, "touch", "move", x, round(y0 + (y1 - y0) * i / n))
        if release:
            self.add(t + dur_ms + 1, "touch", "up", x, y1)

    def wheel_stream(self, t, dur_ms, delta, hz):
        """Wheel events of `delta` at `hz` (120 = one notch; <120 = touchpad-like)."""
        n = int(dur_ms * hz / 1000)
        for i in range(n):
            self.add(t + i * 1000 / hz, "wheel", delta)

    def mouse_drag(self, t, x0, y0, x1, y1, dur_ms, hz=240):
        self.add(t, "move", x0, y0)
        self.add(t + 20, "ldown")
        n = max(1, int(dur_ms * hz / 1000))
        for i in range(1, n + 1):
            self.add(t + 20 + i * 1000 / hz, "move", round(x0 + (x1 - x0) * i / n),
                     round(y0 + (y1 - y0) * i / n))
        self.add(t + 40 + dur_ms, "lup")

    def write(self, path):
        with open(path, "w") as f:
            for t, c in sorted(self.lines, key=lambda x: x[0]):
                f.write(f"{t:.3f} {c}\n")

    @property
    def end_ms(self):
        return max((t for t, _ in self.lines), default=0)


def wait_window(title=TITLE, timeout=20):
    t0 = time.time()
    while time.time() - t0 < timeout:
        r = sh([HARNESS, "window", title])
        if r.returncode == 0:
            return kv(r.stdout)
        time.sleep(0.05)
    raise RuntimeError(f"window {title!r} did not appear")


def app_env(cfg):
    env = dict(os.environ)
    env["PATH"] = str(QT_BIN) + os.pathsep + env["PATH"]
    env["QT_FORCE_STDERR_LOGGING"] = "1"
    for k, v in cfg.items():
        env[k] = str(v)
    return env


def run(name, cfg, script: Script | None, *, mode="pm", settle=1.5, tail=1.5, rects=None,
        marker=None, snaps=(), affinity=None, launch=None, close=True, require_quiet=True,
        clocks=True, pm_target=None):
    """Runs one measured configuration and returns the run directory.

    mode "pm": PresentMon pass (primary timing evidence).
    mode "dd": Desktop Duplication pass (every desktop frame: corruption,
    input-to-photon, clocks). PresentMon 2.6 loses a process's presents while
    a duplication session is active, so the two never run together.
    `launch` replaces the Qt app (baselines): a callable returning (Popen|None, window title).
    Rect, marker and clock coordinates are logical client pixels.
    """
    d = RUNS / name
    d.mkdir(parents=True, exist_ok=True)
    heavy = heavy_processes()
    if heavy and require_quiet:
        raise RuntimeError(f"heavy processes running: {heavy}")
    meta = {"name": name, "mode": mode, "config": cfg, "display": display(), "power_plan": power_plan(),
            "heavy_processes": heavy, "background_cpu_pct": background_cpu(),
            "free_memory_mb": free_memory_mb(), "affinity": affinity,
            "started": time.strftime("%Y-%m-%d %H:%M:%S")}

    if launch is None:
        env = app_env({**cfg, "SPIKE_RING_OUT": d / "ring.csv"})
        proc = subprocess.Popen([str(APP)], env=env, stdout=open(d / "app.log", "w"),
                                stderr=subprocess.STDOUT)
        title = TITLE
    else:
        proc, title = launch()
    win = wait_window(title)
    meta["window"] = win
    if affinity:
        sh(["powershell", "-NoProfile", "-Command",
            f"(Get-Process -Id {win['pid']}).ProcessorAffinity = {affinity}"])
    time.sleep(settle)

    dur = (script.end_ms if script else 0) / 1000 + tail
    if script:
        script.write(d / "input.txt")
    x, y, w, h = (int(win[k]) for k in ("x", "y", "w", "h"))
    k = int(win["dpi"]) / 96
    meta["scale"] = k
    P = lambda v: round(v * k)
    recorders = []
    if mode == "dd":
        rect_args = []
        for rname, (rx, ry, rw, rh) in (rects or {"window": (0, 0, 1280, 800)}).items():
            rect_args += ["--rect", f"{rname},{x + P(rx)},{y + P(ry)},{P(rw)},{P(rh)}"]
        if marker:
            mx, my0, my1 = marker
            rect_args += ["--marker", f"{x + P(mx)},{y + P(my0)},{y + P(my1)}"]
        if clocks:
            rect_args += ["--pixel", f"gui,{x + w - P(16)},{y + h - P(6)}",
                          "--pixel", f"render,{x + w - P(6)},{y + h - P(6)}"]
        for s in snaps:
            rect_args += ["--snap", str(s)]
        recorders.append(subprocess.Popen(
            [str(HARNESS), "dupcap", "--secs", f"{dur + 0.8:.1f}", "--out", str(d / "dupcap"), *rect_args],
            stderr=open(d / "dupcap.log", "w")))
    else:
        recorders.append(subprocess.Popen(
            [str(PRESENTMON), "--process_id", str(pm_target or win["pid"]),
             "--output_file", str(d / "presentmon.csv"), "--timed", f"{dur + 1.5:.1f}",
             "--terminate_after_timed", "--qpc_time", "--no_console_stats", "--stop_existing_session"],
            stdout=subprocess.DEVNULL, stderr=open(d / "presentmon.log", "w")))
    recorders.append(subprocess.Popen(
        [str(HARNESS), "procstat", win["pid"], "250", f"{dur + 1:.1f}", str(d / "proc.csv")]))
    time.sleep(0.8)  # let the recorders start
    if script:
        harness("input", d / "input.txt", d / "input.csv")
    else:
        time.sleep(dur)
    for r in recorders:
        r.wait()
    if close:
        sh(["taskkill", "/PID", win["pid"]])
        if proc is not None:
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                sh(["taskkill", "/F", "/PID", win["pid"]])
    meta["finished"] = time.strftime("%Y-%m-%d %H:%M:%S")
    json.dump(meta, open(d / "meta.json", "w"), indent=2, default=str)
    return d
