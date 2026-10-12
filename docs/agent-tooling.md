# Agent tooling

> **Volatile.** Models, MCP servers and their quirks change every few weeks.
> **Last verified: 2026-10-08.** Re-check this page whenever a tool below
> misbehaves, and update the date when you do. If it is older than a month,
> confirm with the user before relying on it.

## UI driving

- **Delegate UI driving to a cheap model.** In T3 Code, use `delegate_task` with
  the latest GPT "sol" model at `low` or `medium` effort, and have it drive the
  UI through the **`cua-driver` MCP** or **computer-use** (computer-use only works
  while the ChatGPT desktop app is open). This is usually more efficient than a
  Claude thread clicking through the UI itself. Look up the exact model id
  through `orchestrator_capabilities`. Do not hard-code it.
- UI agents are for **functional and visual checks**: does it work, does it look
  right. They are **not** timing instruments. Frame timing and lag come from the
  measurement harness in `HANDOFF.md` (spike A), never from screenshots or an
  agent's impression.

## Cloud test devices

- `gcloud` is installed through scoop on the user's Windows machine and signed in
  as the user's Google account, with project `ristle-spike` as the default.
  The flag is `--no-record-video`; `--record-video=false` is rejected.
- If the gcloud login expires, run `gcloud auth login` in the background and
  have a UI agent click through the consent page in Chrome. Never type
  passwords.
- See the cost guardrails in `HANDOFF.md` before any Test Lab run.

## Known quirks (from Ristle legacy, may be outdated)

- With cua-driver, UIA Select and PostMessage on react-native-windows returned
  success without navigating. Verify with a screenshot and retry foreground
  input after a no-op. Whether the same happens with Qt has not been checked.
- Android emulator UI-hierarchy dumps can be stale during animations. Verify with
  screenshots.
