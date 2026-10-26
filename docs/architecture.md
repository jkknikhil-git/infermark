# Architecture

```text
crates/
  infermark-sdk/   Public traits and types. The only crate modules need to depend on.
  hwprobe/         Hardware detection (CPU, GPU via DXGI / sysfs / system_profiler, NVML, NPU, battery).
  models/          Model catalog, GGUF header reader, hardware gate, verified resumable downloader.
  engine/          Inference backends: llama.cpp (`llama-server` child process) and Ollama (HTTP).
  telemetry/       Background sensor sampler and per-platform sensor providers.
  core/            Registry, planner, runner, profiles, packs, settings, built-in modules and scorers,
                   JSON Schemas.
apps/
  cli/             `infermark` command-line runner.
  desktop/         Tauri 2 app (Rust commands in src-tauri, React + Material 3 UI in src).
catalog/           models.toml (GGUF models) and engines.toml (llama.cpp builds), pinned + hashed.
profiles/          Built-in profiles (compiled into every build).
packs/             Built-in test packs.
schema/            Generated JSON Schemas for result, profile and pack files.
```

## How a run works

```text
Profile (TOML) ─┐
Hardware probe ─┼─► planner::plan ─► [PlanEntry: planned | skipped(reason)] ─► runner::run ─► RunResult (JSON)
Registry ───────┘                                                              │
                                                                              └─► RunEvent stream (CLI / app UI)
```

1. **Profile** says which modules to run, in order, with their settings.
2. **Planner** checks each module exists in this build and that the device meets its
   `Requirement`s (RAM, GPU vendor, battery…). Anything else is skipped with a readable reason.
3. **Runner** runs planned modules one at a time, streams progress events, and records failures
   without stopping the run. A cancel flag stops after the current step.
4. The **RunResult** embeds the hardware, the exact profile and every module's output, so a run can
   be reproduced and compared later.

## Extension points (`infermark-sdk`)

| Trait | Implement it to add… |
| --- | --- |
| `BenchModule` | a benchmark (`descriptor()` + `run(ctx)`) |
| `Backend` | an inference engine (load, streaming generate, stats) |
| `SensorProvider` | a telemetry source (temperatures, clocks, power) |
| `Scorer` | a way to grade answers in test packs |
| `Exporter` | an output format |

Register native modules in `Registry` (see `crates/core/src/modules/mod.rs`). Built-ins can be
overridden by registering a module with the same id.

### Example module

```rust
use infermark_sdk::*;

pub struct HelloModule;

impl BenchModule for HelloModule {
    fn descriptor(&self) -> ModuleDescriptor {
        ModuleDescriptor {
            id: "hello".into(),
            name: "Hello".into(),
            description: "Says hello".into(),
            category: ModuleCategory::Microbenchmark,
            requirements: vec![Requirement::MinRamGb { gb: 4.0 }],
            estimated_seconds: 1,
        }
    }

    fn run(&self, ctx: &RunContext<'_>) -> Result<ModuleOutput, ModuleError> {
        let times = ctx.setting_u64("times", 1); // from [modules.hello] in the profile
        ctx.report(1.0, "done");
        Ok(ModuleOutput {
            metrics: vec![Metric::new("times", "Times", times as f64, "", Direction::Neutral)],
            ..Default::default()
        })
    }
}
```

## Design rules

- Never hard-code hardware; detect it, and report missing sensors as missing.
- Every setting is validated and covered by a test.
- Packs and profiles are data, never code.
- The app and the CLI share the same core, so results are identical.
- Nothing leaves the device unless the user explicitly asks (see [PRIVACY.md](../PRIVACY.md)).

## Running models

```text
catalog/models.toml ──► hardware gate ──► download (HTTPS, allow-listed hosts, SHA-256) ──► models/
                         │  memory = (weights + KV cache) × 1.15
                         │  speed  ≈ measured bandwidth × 0.6 ÷ bytes per token
                         ▼
inference-perf ──► Backend ──► llama-server on 127.0.0.1 (random port + API key)   [llama-cpp]
                           └─► local Ollama HTTP API                               [ollama]
```

llama.cpp runs as the official prebuilt `llama-server` rather than being compiled into InferMark.
That gives every user the right GPU build (Vulkan, CUDA, Metal) without a C++ toolchain, and keeps
the engine version visible in every result. Builds are pinned in `catalog/engines.toml` and
installed into the storage folder; a custom `llama-server` can be set in settings.

Every note and error a module produces is passed through `privacy::redact` before it is saved, so
engine logs can't leak the account name through file paths.

## Telemetry

```text
SensorProvider::channels()  ─► what this device can measure (CPU load, GPU power, battery drain, …)
SensorProvider::sample()    ─► one reading per channel, `None` when it couldn't be read
Sampler (1 s, own thread)   ─► bounded series (thinned evenly past 1200 points)
RunResult.telemetry         ─► { sensors: { available, missing }, interval_ms, series }
```

Devices expose very different sensors: Windows usually hides CPU temperature from normal
users, NVIDIA GPUs report power through NVML, Android phones give battery current and a thermal
status, iPhones only a coarse thermal state. A sensor that doesn't exist is listed under
`sensors.missing`, and an unreadable value is skipped, so charts can say "not available on this
device" instead of drawing zeros. Desktop providers live in `crates/telemetry`; phones will add
their own through a Tauri mobile plugin and pass them to `core::run_with_sensors`. Try
`cargo run -p infermark-telemetry --example sensors` to see what a machine offers.
