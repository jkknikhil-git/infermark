# Contributing to InferMark

Thanks for helping build an open benchmark for local AI. So, first of all: you don't need to know
Rust to make a real difference here. Test packs, profiles and catalog entries are plain data, and
results from devices we haven't tried are just as valuable as code.

## Ways to contribute

1. **Test packs** (no Rust needed). Add a folder under `packs/` with a `pack.toml` and `items.jsonl`,
   then run `cargo run -p infermark-cli -- pack validate packs/<your-pack>` before opening a PR.
   Only contribute items you wrote yourself or that come from a dataset whose license allows
   redistribution, and put that license in `pack.toml`. [docs/packs.md](docs/packs.md) walks
   through the format.
2. **Profiles.** Presets for specific use cases (e.g. "thin-and-light laptop", "gaming desktop").
   Check yours with `cargo run -p infermark-cli -- profile validate <file.toml>`.
3. **Models.** Entries for `catalog/models.toml`: a single-file GGUF on Hugging Face, pinned to a
   commit, with its SHA-256. [catalog/README.md](catalog/README.md) has a tool that writes the
   entry for you.
4. **Native modules.** New backends, sensors, scorers or exporters behind a Cargo feature. Read
   [docs/architecture.md](docs/architecture.md) first.
5. **Results and bug reports.** Attach your `result.json` from `~/.infermark/results`. It contains
   hardware details, but no username, hostname or file paths.

## Development setup

Here's what you need:

- Rust (stable, via rustup) with the MSVC toolchain on Windows. MSRV is 1.80.
- Node.js 20+ for the desktop app (CI uses 22).
- Only for the in-process llama.cpp engine (`--features infermark-engine/llama-inproc`): CMake,
  a C++ compiler and libclang. CI tests that feature on Linux only.

You don't need the Vulkan SDK or the CUDA toolkit. GPU support comes from the prebuilt
`llama-server` builds that `cargo run -p infermark-cli -- engine install` downloads (Vulkan, CUDA,
Metal or CPU, pinned in `catalog/engines.toml`).

Run these before every commit; they match CI:

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
cargo run -q -p infermark-cli -- schema export --dir schema   # then commit any schema/ changes
```

For the desktop UI, `npm run build` in `apps/desktop` typechecks and builds it. `npm run dev`
serves it in a plain browser on port 1420 with the preview data in `src/sample.ts`, so you can
work on layouts without a benchmark running.

## Ground rules

These are short, and they're what keep results trustworthy:

- Every CLI flag and profile setting must be wired up **and** covered by a test.
- Never hard-code hardware; detect it.
- Missing sensors are reported as missing, never as zero.
- Write all files as UTF-8.
- Keep the result schema backwards compatible within a major version: new fields get
  `#[serde(default)]`, and you regenerate `schema/*.json` with
  `cargo run -p infermark-cli -- schema export` when types change.

## Commit messages

Short imperative subject (`Add memory bandwidth module`), with detail in the body when useful.

That's it. If something here is unclear or out of date, that's a bug too, and a PR fixing it is
very welcome.
