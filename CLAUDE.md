# CLAUDE.md

See `AGENTS.md` for conventions, `CONTEXT.md` for the domain model and `HANDOFF.md`
for the current phase.

Ristle legacy, the previous app, is at `../Ristle`. Its code and tests are a
reference for *behavior*: the C++ tests under `packages/core/tests` and
`packages/app-core/tests` are the spec for the Rust port. Its architecture is not
a template, and nothing from it is to be treated as disposable or "good enough
for now". Its `CLAUDE.md` describes React Native, SpacetimeDB and bridge problems
that do not apply here.

## Words

Only call something a *prototype*, *spike* or *throwaway* when it really will be
deleted. Spikes live in `spikes/` and are deleted once their findings are written
up. Everything else is production code and is built to that standard from the
first commit.

## Volatile tooling: check `docs/agent-tooling.md`

UI driving should be delegated to a cheap GPT "sol" model through `cua-driver` or
computer-use. Details and the last-verified date are in `docs/agent-tooling.md`.
That page goes stale quickly. If its date is more than a month old, ask the user
before relying on it, and update it when you learn something new.

## Next-run orientation

- **Git Bash rewrites leading-slash API paths.** Use `gh api users/...`, not
  `gh api /users/...`. Reading GitHub billing needs the `user` scope, which the
  current `gh` login lacks.
- **wasmtime ≥ 49 needs Rust ≥ 1.96.** The global toolchain is older, so pin it
  per workspace with `rust-toolchain.toml` rather than updating it globally.
  wasmtime's own `Result`/`Error` do not convert from or to `anyhow`, so use
  `wasmtime::Result`.
- **The iOS Simulator hides address-space limits.** Anything that reserves large
  virtual memory (wasmtime memories) passes there and fails on a device. Real
  device runs go through Firebase Test Lab (`spikes/c-wasm-ios/README.md`).
- cmake and ninja for any native build ship inside Visual Studio and are not on
  PATH. Resolve them with `vswhere` (see `../Ristle/CLAUDE.local.md`).
- **WIT tooling** is installed with `cargo install --locked wasm-tools wit-bindgen-cli`.
  Validate with `wasm-tools component wit plugin-api/wit` and
  `wit-bindgen rust --world full-provider --out-dir <tmp> plugin-api/wit` (the
  WIT path goes last). In WIT, `from` is a keyword (write `%from`), and a
  function cannot share a name with a type in the same interface.
- **Bash heredocs break on `'''`.** A quoted heredoc containing Python
  triple-single-quotes fails with "unexpected EOF". Write the script to a file
  with the Write tool and run it instead.
