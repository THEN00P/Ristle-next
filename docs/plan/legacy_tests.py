#!/usr/bin/env python3
"""The phase 2 exit checklist for porting Ristle legacy's tests.

    python docs/plan/legacy_tests.py extract  [--legacy PATH]
    python docs/plan/legacy_tests.py check    [--legacy PATH]
    python docs/plan/legacy_tests.py progress [--strict]

`extract` lists every legacy test case. `check` compares that list with the
inventory in docs/plan/phase-2.md and fails on any test the plan does not
place, or names that no longer exist. `progress` looks up every `port` row's
Rust test function in its crate under core/crates and reports which are
still missing; `--strict` fails while any are, which is the phase exit.

The legacy checkout defaults to `../Ristle` next to this repository, or
`$RISTLE_LEGACY`. Needs only the Python standard library.
"""

import argparse
import os
import pathlib
import re
import sys

REPO = pathlib.Path(__file__).resolve().parents[2]
PLAN = REPO / "docs" / "plan" / "phase-2.md"
CRATES = REPO / "core" / "crates"

# The suites that make up the checklist, relative to Ristle legacy's root.
CPP_SUITES = [
    "packages/core/tests",
    "packages/app-core/tests",
    "packages/audio-engine/tests",
]
# The Rust half of legacy Party, linked into app-core: unit and integration tests.
RUST_SUITES = ["packages/party-client"]
# Test executables with a `main` instead of registered cases.
EXECUTABLES = {"packages/app-core/tests/party_transport_smoke.cpp": "party_transport_smoke"}

CPP_TEST = re.compile(r"^RISTLE_TEST\(\s*(\w+)\s*\)", re.MULTILINE)
RUST_TEST = re.compile(r"#\[(?:tokio::)?test[^\]]*\]\s*(?:#\[[^\]]*\]\s*)*(?:async\s+)?fn\s+(\w+)")
ROW = re.compile(
    r"^\| `(?P<name>\w+)` \| (?P<status>port|retire|phase 5) \|"
    r" (?:`(?P<crate>[\w-]+)`)? *\| (?P<milestone>\w*) *\| (?P<rest>.*)\|$"
)


def extract(legacy: pathlib.Path) -> dict[str, str]:
    """Maps each legacy test name to its file."""
    if not (legacy / "packages" / "core" / "tests").is_dir():
        sys.exit(f"no Ristle legacy checkout at {legacy}; pass --legacy or set RISTLE_LEGACY")
    tests: dict[str, str] = {}

    def add(name: str, path: str) -> None:
        if name in tests:
            sys.exit(f"duplicate legacy test name {name} in {path} and {tests[name]}")
        tests[name] = path

    for suite in CPP_SUITES:
        for path in sorted((legacy / suite).glob("*.cpp")):
            relative = path.relative_to(legacy).as_posix()
            for name in CPP_TEST.findall(path.read_text(encoding="utf-8")):
                add(name, relative)
            if relative in EXECUTABLES:
                add(EXECUTABLES[relative], relative)
    for suite in RUST_SUITES:
        for path in sorted((legacy / suite).rglob("*.rs")):
            relative = path.relative_to(legacy).as_posix()
            for name in RUST_TEST.findall(path.read_text(encoding="utf-8")):
                add(name, relative)
    return tests


def inventory() -> dict[str, dict[str, str]]:
    rows: dict[str, dict[str, str]] = {}
    for line in PLAN.read_text(encoding="utf-8").splitlines():
        match = ROW.match(line)
        if not match:
            continue
        row = match.groupdict()
        if row["name"] in rows:
            sys.exit(f"{row['name']} appears twice in {PLAN.name}")
        if row["status"] == "port":
            rust = re.fullmatch(r" *`(\w+)` *", row["rest"])
            if not (row["crate"] and row["milestone"] and rust):
                sys.exit(f"port row for {row['name']} needs a crate, a milestone and a Rust test name")
            row["rust"] = rust.group(1)
        rows[row["name"]] = row
    return rows


def rust_tests(crate: str) -> set[str]:
    names: set[str] = set()
    for path in (CRATES / crate).rglob("*.rs"):
        names.update(RUST_TEST.findall(path.read_text(encoding="utf-8")))
    return names


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("command", choices=["extract", "check", "progress"])
    parser.add_argument(
        "--legacy",
        type=pathlib.Path,
        default=pathlib.Path(os.environ.get("RISTLE_LEGACY", REPO.parent / "Ristle")),
    )
    parser.add_argument("--strict", action="store_true")
    args = parser.parse_args()

    if args.command == "extract":
        for name, path in extract(args.legacy).items():
            print(f"{path}\t{name}")
        return 0

    rows = inventory()
    if args.command == "check":
        legacy = extract(args.legacy)
        missing = sorted(set(legacy) - set(rows))
        unknown = sorted(set(rows) - set(legacy))
        for name in missing:
            print(f"not in the plan: {legacy[name]} {name}")
        for name in unknown:
            print(f"not in legacy:   {name}")
        counts = {s: sum(r["status"] == s for r in rows.values()) for s in ("port", "retire", "phase 5")}
        print(f"{len(legacy)} legacy tests; plan: {counts}")
        return 1 if missing or unknown else 0

    found: dict[str, set[str]] = {}
    pending = []
    for name, row in rows.items():
        if row["status"] != "port":
            continue
        crate = row["crate"]
        if crate not in found:
            found[crate] = rust_tests(crate) if (CRATES / crate).is_dir() else set()
        if row["rust"] not in found[crate]:
            pending.append((row["milestone"], crate, row["rust"], name))
    ported = sum(r["status"] == "port" for r in rows.values()) - len(pending)
    for milestone, crate, rust, name in sorted(pending):
        print(f"pending {milestone:4} {crate:9} {rust}  ({name})")
    print(f"ported {ported} of {ported + len(pending)}")
    return 1 if args.strict and pending else 0


if __name__ == "__main__":
    sys.exit(main())
