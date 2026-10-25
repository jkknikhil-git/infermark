# Contributing to InferMark

Thanks for helping build an open benchmark for local AI!

## Ways to contribute

1. **Test packs** (no Rust needed) – add a folder under `packs/` with a `pack.toml` and `items.jsonl`.
   Run `cargo run -p infermark-cli -- pack validate packs/<your-pack>` before opening a PR.
   Only contribute items you wrote yourself or that come from a dataset whose license allows
   redistribution; put the license in `pack.toml`.
2. **Profiles** – presets for specific use cases (e.g. "thin-and-light laptop", "gaming desktop").
3. **Models** – entries for `catalog/models.toml` (link to the GGUF on Hugging Face plus its SHA-256).
4. **Native modules** – new backends, sensors, scorers or exporters behind a Cargo feature.
5. **Results and bug reports** – attach your `result.json`; it contains no personal data beyond
   hardware details.

## Development setup

- Rust (stable, via rustup) with the MSVC toolchain on Windows
- CMake (needed for llama.cpp from M1)
- Node.js 20+ for the desktop app
- Optional: Vulkan SDK (GPU builds on AMD/Intel), CUDA toolkit (NVIDIA)

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Ground rules

- Every CLI flag and profile setting must be wired up **and** covered by a test.
- Never hard-code hardware; detect it.
- Missing sensors are reported as missing, never as zero.
- Write all files as UTF-8.
- Keep the result schema backwards compatible within a major version; regenerate
  `schema/*.json` with `cargo run -p infermark-cli -- schema export` when types change.

## Commit messages

Short imperative subject (`Add memory bandwidth module`), with detail in the body when useful.
