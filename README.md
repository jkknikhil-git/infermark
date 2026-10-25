<p align="center">
  <img src="apps/desktop/app-icon.svg" width="96" alt="InferMark logo">
</p>

<h1 align="center">InferMark</h1>

<p align="center">
  <b>The open-source benchmark for local AI.</b><br>
  Find out how well your laptop, desktop or phone really runs local language models —
  speed, responsiveness, heat, battery and answer quality — in one run.
</p>

---

> **Status:** early development (milestone M0). Expect breaking changes. Star/watch the repo to follow along.

## Why InferMark?

Geekbench and 3DMark tell you how fast your CPU and GPU are. They don't tell you whether your machine can
run a 7B coding assistant at a usable speed, how much it throttles after 15 minutes of generation, or how
many hours of local chat you get on battery. InferMark does.

- **Run once, test everything** – hardware probe, memory bandwidth, prompt processing, generation speed,
  time-to-first-token, sustained/thermal throttling, battery drain, and quality across coding,
  reasoning, summarization, creative writing and more.
- **Hardware-aware** – models your device can't realistically run are filtered out *before* anything
  is downloaded, with a clear reason ("needs 42 GB, you have 16 GB").
- **Modular** – choose what runs with profiles, add your own tests as simple *packs* (TOML + JSONL),
  or write native modules for new backends and sensors.
- **Native app** – install it, press Run, watch live graphs, keep a history of your results.
- **Open** – Apache-2.0, reproducible results, versioned result schema.

## What's measured

| Area | Metrics |
| --- | --- |
| System | CPU, GPU, NPU, RAM, OS, drivers, power source |
| Microbenchmarks | Memory bandwidth, CPU/GPU compute |
| Inference | Prompt processing tok/s, generation tok/s, time to first (visible) token, p50/p95 latency, context scaling |
| Endurance | Sustained throughput, throttle %, temperatures, clocks, power |
| Battery | % per 10k tokens, tokens per Wh, estimated hours of chat |
| Quality | Coding, reasoning, knowledge, instruction following, structured output, summarization, creative, long context |

Results roll up into Speed, Responsiveness, Endurance, Efficiency and Quality sub-scores and a single
**InferMark Score** (reference device = 1000).

## Quick start (CLI, from source)

```bash
cargo run -p infermark-cli -- probe            # what hardware do I have?
cargo run -p infermark-cli -- plan --profile quick   # what would run, and what gets skipped (and why)?
cargo run -p infermark-cli -- run --profile quick    # run it and write a result JSON to ./results
```

Desktop app (Tauri 2):

```bash
cd apps/desktop
npm install
npm run tauri dev
```

## Make it yours

| Layer | For | How |
| --- | --- | --- |
| **Profiles** | Everyone | TOML files in `profiles/` or `~/.infermark/profiles/` – pick modules, repeats, thresholds, score weights. |
| **Test packs** | Anyone who can write JSON | A folder with `pack.toml` + `items.jsonl` in `packs/` or `~/.infermark/packs/`. See [docs/packs.md](docs/packs.md). |
| **Native modules** | Rust developers | Implement a trait from `crates/infermark-sdk` (`BenchModule`, `Backend`, `SensorProvider`, `Scorer`, `Exporter`). See [docs/architecture.md](docs/architecture.md). |

## Roadmap

- **M0** – Scaffold, SDK traits, profiles & packs, hardware probe, desktop shell ← *we are here*
- **M1** – llama.cpp + Ollama backends, model catalog & downloader, hardware gate, inference benchmarks
- **M2** – Telemetry (temps, clocks, power, battery), live charts, sustained & battery tests
- **M3** – Quality packs and scorers, code sandbox, LLM-as-judge
- **M4** – v1.0 desktop release with installers and auto-update
- **M5+** – Android, iOS, public leaderboard, WASM plugins

## Contributing

Contributions are very welcome – new test packs are the easiest place to start. Read
[CONTRIBUTING.md](CONTRIBUTING.md) and our [Code of Conduct](CODE_OF_CONDUCT.md).

## License

[Apache-2.0](LICENSE). Model weights and datasets keep their own licenses; InferMark never redistributes
model weights.
