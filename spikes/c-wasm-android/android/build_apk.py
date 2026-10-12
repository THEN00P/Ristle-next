"""Builds the spike APK from the SDK command-line tools, without Gradle.

Usage: python android/build_apk.py [abi ...]   (default: x86_64)
Expects target/<triple>/release/libplugin_host_spike.so from `cargo ndk`.
"""

import os
import shutil
import subprocess
import sys
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent
WORKSPACE = ROOT.parent
SDK = Path(os.environ.get("ANDROID_HOME") or Path(os.environ["LOCALAPPDATA"]) / "Android" / "Sdk")
BUILD_TOOLS = SDK / "build-tools" / "36.1.0"
ANDROID_JAR = SDK / "platforms" / "android-36" / "android.jar"
TRIPLES = {"x86_64": "x86_64-linux-android", "arm64-v8a": "aarch64-linux-android"}
OUT = WORKSPACE / "target" / "apk"


def tool(name):
    for ext in ("", ".exe", ".bat"):
        p = BUILD_TOOLS / (name + ext)
        if p.exists():
            return str(p)
    raise SystemExit(f"missing build tool {name}")


def run(*args):
    subprocess.run([str(a) for a in args], check=True)


def main():
    abis = sys.argv[1:] or ["x86_64"]
    shutil.rmtree(OUT, ignore_errors=True)
    classes = OUT / "classes"
    classes.mkdir(parents=True)
    run("javac", "--release", "11", "-classpath", ANDROID_JAR, "-d", classes,
        ROOT / "src" / "org" / "ristle" / "spike" / "SpikeActivity.java")
    run(tool("d8"), "--release", "--lib", ANDROID_JAR, "--output", OUT, *classes.rglob("*.class"))
    base = OUT / "base.apk"
    run(tool("aapt2"), "link", "-o", base, "--manifest", ROOT / "AndroidManifest.xml", "-I", ANDROID_JAR,
        "--min-sdk-version", "26", "--target-sdk-version", "35")
    with zipfile.ZipFile(base, "a", zipfile.ZIP_DEFLATED) as z:
        z.write(OUT / "classes.dex", "classes.dex")
        for abi in abis:
            so = WORKSPACE / "target" / TRIPLES[abi] / "release" / "libplugin_host_spike.so"
            z.write(so, f"lib/{abi}/libplugin_host_spike.so")
    aligned = OUT / "aligned.apk"
    run(tool("zipalign"), "-f", "4", base, aligned)
    final = OUT / "ristle-spike-c.apk"
    keystore = Path.home() / ".android" / "debug.keystore"
    run(tool("apksigner"), "sign", "--ks", keystore, "--ks-pass", "pass:android", "--out", final, aligned)
    print(final)


if __name__ == "__main__":
    main()
