# ADR 0011: CI from day one

## Status

Accepted (2026-10-08)

## Context

Ristle legacy had no CI. Review comments were the only signal, and platform
builds were checked by hand when someone remembered.

## Decision

GitHub Actions runs on every push and PR:

- `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test` for the
  workspace, and `cargo deny` for licences and advisories.
- Builds for every app that exists: Android (Gradle), desktop (Qt on Windows and
  macOS runners), and iOS (Xcode on macOS runners) once it exists.
- Plugin API: build the example Plugin as a component and run the host
  conformance tests against it.
- Server: integration tests against a PostgreSQL service container.

## Consequences

- A component is not done until its CI job exists.
- macOS runners cost more. Run Apple jobs on PRs and the default branch, not on
  every push to a feature branch, if minutes run short.
