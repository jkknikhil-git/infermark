# Built-in test packs

Each folder here is a test pack: a `pack.toml` that says what the pack is and how answers are
scored, plus an `items.jsonl` with one question per line. Packs are plain data, so you can write
one without knowing Rust, and they're never executed as code. [docs/packs.md](../docs/packs.md)
covers the format.

Here's the honest state of things: this folder is empty for now. The first built-in packs, and
the quality scores that use them, arrive with milestone M3. Until then, profiles that list
`quality-packs` show it as skipped in `infermark plan`.

If you want to get a head start, write a pack and check it with
`cargo run -p infermark-cli -- pack validate packs/<your-pack>`.
