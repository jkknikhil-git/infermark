<p align="center">
  <img src="apps/desktop/app-icon.svg" width="96" alt="InferMark logo">
</p>

<h1 align="center">InferMark</h1>

<p align="center">
  <b>The open-source benchmark for local AI.</b><br>
  Find out how well your laptop or desktop really runs local language models, measured on the
  device itself. Phones come later.
</p>

---

> **Status:** early development. M1 is done and M2 is in progress, so expect breaking changes.
> Star or watch the repo to follow along.

## So, what does it tell you?

InferMark answers one question: how well does *this* device run a local model? It checks the
hardware, tells you which catalog models fit before you download anything, verifies every download
by SHA-256, then measures the models you have. Your results stay on the device.

Here's a real run. On an Intel Core i5-11320H laptop with Iris Xe graphics and 16 GB of RAM,
Qwen3 0.6B through llama.cpp's Vulkan build generates about **45 tok/s**, reads the prompt at
about 480 tok/s, and puts the first token on screen after about 1.4 s. On the same laptop,
Llama 3.3 70B never gets downloaded at all, and the gate tells you why:
"Needs 47.0 GB, this device can use 13.4 GB".

One honest caveat: that same laptop has given anywhere from 18 to 45 tok/s across six runs. A
single number is one data point, which is why every run records repeats, free memory and power
state alongside the speed.

## Why not just use Geekbench?

And look, Geekbench and 3DMark are good at what they do. They tell you how fast your CPU and GPU
are, and they've had years to get that right. But they don't answer the questions you actually
have about local AI:

- Can this machine run a 7B coding assistant at a usable speed?
- How much does it throttle after 15 minutes of generation?
- How many hours of local chat do you get on battery?

InferMark answers the first one today. The other two are what M2 is for.

Here's how it's put together:

- **Hardware-aware.** Models the device can't realistically run are filtered out *before* anything
  is downloaded, with the reason spelled out.
- **Modular.** Profiles choose what runs, test *packs* are plain TOML + JSONL, and native modules
  add new backends and sensors.
- **Open.** Apache-2.0, reproducible results and a versioned result schema. Every result embeds
  the hardware and the exact profile it ran.

## What's measured today

| Area | Metrics |
| --- | --- |
| System | OS, CPU (cores and threads), RAM and free memory, GPUs and drivers, NPUs, battery, power source |
| Memory | Copy and triad bandwidth (GB/s) |
| Inference | Prompt processing tok/s, generation tok/s, time to first token, load time. Medians over the profile's repeats, after warm-up runs. Backends: llama.cpp (`llama-server`, or in-process with the `llama-inproc` feature) and Ollama |
| Telemetry | Sampled every second during the run: load, clocks, temperatures, power and battery charge, where the device exposes them. A sensor the device doesn't have is listed as missing, never drawn as zero |

## What's coming

| Area | Metrics | Milestone |
| --- | --- | --- |
| Endurance | Sustained throughput, throttle %, temperatures, clocks, power | M2 |
| Battery | % per 10k tokens, tokens per Wh, estimated hours of chat | M2 |
| Quality | Coding, reasoning, knowledge, instruction following, structured output, summarization, creative, long context | M3 |
| Inference | p50/p95 latency, context scaling | Later |
| Microbenchmarks | CPU/GPU compute | Later |

Results will roll up into Speed, Responsiveness, Endurance, Efficiency and Quality sub-scores and a
single **InferMark Score** (reference device = 1000). Profiles already carry the score weights;
nothing computes the score yet.

## Quick start (CLI, from source)

```bash
cargo run -p infermark-cli -- probe                    # what hardware do I have?
cargo run -p infermark-cli -- models                   # which catalog models fit this device, and how fast?
cargo run -p infermark-cli -- models pull qwen3-0.6b-q4  # download one (resumable, SHA-256 checked)
cargo run -p infermark-cli -- engine install           # install the llama.cpp build for this device
cargo run -p infermark-cli -- plan --profile quick     # what would run, and what gets skipped (and why)?
cargo run -p infermark-cli -- run --profile quick      # run it and write a result JSON to ~/.infermark/results
```

A run only uses models you've already downloaded; it never fetches one on its own. By default it
prefers llama.cpp when it's installed and has a model to run, and falls back to a local Ollama. The `quick` profile takes about 5 minutes, and
`cargo run -p infermark-cli -- --help` lists the rest (packs, settings and schemas).

Desktop app (Tauri 2):

```bash
cd apps/desktop
npm install
npm run tauri dev
```

## Make it yours

| Layer | For | How |
| --- | --- | --- |
| **Profiles** | Everyone | TOML files in `profiles/` or `~/.infermark/profiles/`: pick modules, repeats, thresholds, score weights. Check one with `cargo run -p infermark-cli -- profile validate <file.toml>`. |
| **Test packs** | Anyone who can write JSON | A folder with `pack.toml` + `items.jsonl` in `packs/` or `~/.infermark/packs/`. See [docs/packs.md](docs/packs.md). |
| **Native modules** | Rust developers | Implement a trait from `crates/infermark-sdk` (`BenchModule`, `Backend`, `SensorProvider`, `Scorer`, `Exporter`). See [docs/architecture.md](docs/architecture.md). |

## Roadmap

- **M0** – Scaffold, SDK traits, profiles & packs, hardware probe, desktop shell. Done.
- **M1** – llama.cpp + Ollama backends, model catalog & downloader, hardware gate, inference benchmarks. Done.
- **M2** – Telemetry (temps, clocks, power, battery), live charts, sustained & battery tests ← *we are here*. The sensor sampler is in; the charts and the two tests are being built.
- **M3** – Quality packs and scorers, code sandbox, LLM-as-judge
- **M4** – v1.0 desktop release with installers and auto-update
- **M5+** – Android, iOS, public leaderboard, WASM plugins

## Contributing

So, if you want to help, new test packs are the easiest place to start: no Rust needed, just TOML
and JSON. Read [CONTRIBUTING.md](CONTRIBUTING.md) and our [Code of Conduct](CODE_OF_CONDUCT.md).
Results from devices we haven't tried yet are just as useful.

## License

[Apache-2.0](LICENSE). Model weights and datasets keep their own licenses; InferMark never redistributes
model weights.

What leaves the device, and when, is spelled out in [PRIVACY.md](PRIVACY.md). Short version:
nothing, unless you ask for it.
