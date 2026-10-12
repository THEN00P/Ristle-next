# Working in Ristle

Read `CONTEXT.md` for the domain language and `HANDOFF.md` for the current phase.
Decisions live in `docs/adr/`. Do not reverse an accepted ADR silently. If work
contradicts one, say so and propose an amendment.

## Ownership

- **The Rust core owns all behavior and state.** That covers queue, sequences,
  playback, persistence, caching, Provider orchestration, validation and Party
  logic. A UI renders core state and sends user intentions, nothing more.
- **UIs hold no domain state.** No queue, shuffle order, playback position or
  Provider data may be owned or persisted in Kotlin, Swift or QML. View state
  (scroll position, which sheet is open) is fine.
- **QML contains no logic.** Bindings stay simple expressions and handlers call one
  core method. Anything longer belongs in Rust behind a model or method.
- **Platform code is limited to platform concerns.** That means OS media sessions,
  audio-session policy, permissions, credential storage, web views for sign-in,
  file pickers and notifications.
- **One interface definition per boundary, generated rather than handwritten:**
  UniFFI for core↔Kotlin and core↔Swift, cxx-qt for core↔Qt, WIT for
  core↔Plugin, and one shared Rust crate for client↔server protocol types.

## Plugins

- Plugins never decode audio (ADR 0004). They resolve sources and may supply or
  transform bytes. The host demuxes, decodes, resamples and plays.
- Every Plugin capability goes through the WIT interface. No ambient authority.
- The app repository contains only the Local Files Plugin and example Plugins.
  Provider Plugins for third-party services live in their own repositories
  (ADR 0010).
- Provider terms are followed where the design allows, through generic
  manifest switches rather than Provider-specific host code. ADR 0012 has
  Ristle's reading of Spotify's terms (no overlap at Spotify boundaries, the
  Provider always shown, only Track references relayed to Party guests).

## UI

Follow `docs/ui/guidelines.md`. In short: show the Provider once per context, use
at most one row of chips per screen, keep badges for states that change a
decision, keep desktop and mobile layouts separate, and never show an empty
placeholder.

## Verification

CI runs on every push (ADR 0011). A change is done when CI is green and the
behavior was exercised: a test for core logic, and a run on the affected
platform for UI or platform work. Say what you did not verify.
