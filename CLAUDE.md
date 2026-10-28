# InferMark

Open-source, local-first benchmark for running AI models (SLMs/LLMs) on your own PC, and later
phones: hardware probe, a gate that drops models too big for the device, verified model
downloads, inference speed, endurance/thermals, battery and answer quality. The product is the
installable app; results stay on the device.

Layout and data flow are in [docs/architecture.md](docs/architecture.md); read it before
changing how a run works.

## Commands

Run these before every commit (they match CI):

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
cargo run -q -p infermark-cli -- schema export --dir schema   # then commit any schema/ changes
```

The desktop UI is in `apps/desktop` (`npm run build` typechecks and builds). `npm run dev`
(launch config `desktop-ui`, port 1420) runs it in a plain browser on the preview data in
`src/sample.ts`, so keep that file in step with new events and result fields.

The in-process llama.cpp engine is behind `--features infermark-engine/llama-inproc`. It builds
llama.cpp from source (CMake, a C++ compiler, libclang) and CI tests it on Linux only.

## Rules

- **Privacy.** No telemetry, analytics or accounts. The network is only used for downloads the
  user starts. Results contain no username, hostname or absolute paths: module notes and errors
  go through `privacy::redact`, and local models are named by file name only.
- **Downloads.** HTTPS to allow-listed hosts only, SHA-256 checked before use; new catalog
  entries (`catalog/*.toml`) are pinned to a commit and carry a hash.
- **Hardware.** Never hard-code it; detect it. A sensor a device doesn't have is listed as
  missing and an unreadable value is `None`, never a zero.
- **Modules** implement `BenchModule` from `infermark-sdk` and are registered in
  `crates/core/src/modules/mod.rs`. They report progress, stop on cancel, and return
  `ModuleError::Skipped` (not `Failed`) when there is nothing to do on this device.
- **Settings.** Every profile setting and CLI flag is validated, wired up and covered by a test.
  Packs and profiles are data, never code.
- **Results.** Keep `result.v1` backwards compatible: new fields get `#[serde(default)]`.
- **Phones later.** Code under `crates/` must not assume a desktop: no child processes on the
  mobile path, sensors come through `SensorProvider`, and memory margins are per OS.
- **Dependencies.** Workspace crates are `publish = false`; MSRV is 1.80 (no newer std APIs);
  `cargo-deny` runs in CI, so check advisories when adding or upgrading a crate.

## Writing

Prose (README, docs, UI copy, release notes, PR descriptions) is written in the voice of a calm,
reasonable tech reviewer who has actually run the thing:

- Talk to the reader ("you", "we") and state the main point first.
- Back every claim with something concrete: a number, a real test result, a named device.
  "45 tok/s on Iris Xe", not "fast on most laptops".
- Give the other side credit before disagreeing, and separate what's measured from what's
  still unknown.
- Use signposts ("So, first of all…", "And look…", "Here's the thing…") and the occasional
  triplet. No padding, no hype, never ranty.
- End sections with practical advice and a light, humble close rather than a sales pitch.

Keep code comments, error messages, commit subjects (short imperative), schemas and the policy
text in PRIVACY.md and SECURITY.md plain and exact.

## UI

Material 3 look using the tokens in `apps/desktop/src/styles.css`; no generic "AI" styling.
Phone first: four destinations (Home, Results, Models, Settings) with the run as a full-screen
flow, touch targets of 48 px or more, and safe-area padding. Check layouts at 360, 390 and
412 px wide and at desktop width, with no horizontal scrolling. Say "this device", not
"your computer". Charts mark missing sensors as "not available on this device".
