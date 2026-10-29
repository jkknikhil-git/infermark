# Architecture

```text
crates/
  infermark-sdk/   Public traits and types. The only crate modules need to depend on.
  hwprobe/         Hardware detection (CPU, GPU via DXGI / sysfs / system_profiler, NVML, NPU, battery).
  models/          Model catalog, GGUF header reader, hardware gate, verified resumable downloader.
  engine/          Inference backends: llama.cpp (`llama-server` child process, or in-process with the
                   `llama-inproc` feature) and Ollama (HTTP).
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
3. **Runner** first runs the pre-run checks (`preflight.rs`, see below), then runs planned modules
   one at a time, streams progress events, and records failures without stopping the run. A
   cancel flag stops after the current step.
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
                           ├─► llama.cpp linked in-process (feature llama-inproc)  [llama-cpp-inproc]
                           └─► local Ollama HTTP API                               [ollama]
```

llama.cpp runs as the official prebuilt `llama-server` rather than being compiled into InferMark.
That gives every user the right GPU build (Vulkan, CUDA, Metal) without a C++ toolchain, and keeps
the engine version visible in every result. Builds are pinned in `catalog/engines.toml` and
installed into the storage folder; a custom `llama-server` can be set in settings.

Phones can't do that: Android 10+ won't execute files from app storage and iOS doesn't allow child
processes. The `llama-inproc` Cargo feature of `infermark-engine` links llama.cpp into the process
instead (through `llama-cpp-2`, built from source, so it needs CMake, a C++ compiler and libclang).
It behaves like the `llama-server` backend: the same prompt layout and default sampler, stop
strings, `ignore_eos`, a fresh KV cache for every run, and the vendored llama.cpp build in the
version string. It is off by default; CI builds it on Linux and generates tokens with a 27 MB model.

Every note and error a module produces is passed through `privacy::redact` before it is saved, so
engine logs can't leak the account name through file paths.

The gate's memory budget is a share of total memory: 85% on desktops, 55% on Android and iOS.
That gap is deliberate. A desktop will happily page memory out, but a phone closes the app long
before memory is full, so a model that "fits" at 85% gets killed mid-test. A profile can set
`gate.memory_margin` to override both.

### Cores and threads

`HardwareInfo.cpu.clusters` lists groups of identical cores, fastest first. Windows gets them
from each core's efficiency class, Linux and Android from every core's highest clock in
`/sys/devices/system/cpu`, and macOS from `hw.perflevelN`. Most desktops have one group; the
i5-11320H this was built on reads as 4 cores, 8 threads. An Intel hybrid chip has two, and a
phone has two or three (prime, big, little).

`thread-sweep` uses that layout to answer a practical question: how many threads should
llama.cpp get? It runs the smallest model on the CPU alone (GPU off) with the fast cores only
(when there's more than one kind), every core, and every thread (when there are more threads
than cores), then reports the fastest. More isn't automatically better here. Generation is
limited by memory bandwidth, and little cores can hold the big ones back.

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
device" instead of drawing zeros. Desktop providers live in `crates/telemetry`. On Android the
app adds `AndroidProvider` (`apps/desktop/src-tauri/src/android.rs`), which reads a Kotlin plugin
in `gen/android` (`DevicePlugin.kt`) and passes it to `core::run_with_sensors`. Try
`cargo run -p infermark-telemetry --example sensors` to see what a machine offers.

The standard channels (`STANDARD_CHANNELS` in `sensor.rs`) are the ones a results screen always
accounts for, as either a chart or a "not available on this device" line:

| Key | Unit | Desktop source today |
|---|---|---|
| `cpu.utilization`, `cpu.clock` | %, MHz | sysinfo, everywhere |
| `cpu.temperature` | °C | sysinfo where the OS shares it (Linux yes; Windows needs admin, so missing) |
| `cpu.power` | W | Linux RAPL package zones, only when `energy_uj` is readable (often root-only) |
| `gpu.utilization`, `gpu.temperature`, `gpu.power` | %, °C, W | NVML on NVIDIA; other GPUs missing for now |
| `battery.battery-charge`, `battery.power` | %, W | hwprobe's battery reader; drain only while discharging |
| `battery.temperature` | °C | the same, on the few laptops that report it |
| `device.thermal-state` | 0-3 | Android `PowerManager` thermal status (the app build); none on desktop, iOS later |

On Android, battery charge, drain (current × voltage while discharging) and battery temperature
come from `BatteryManager`, and CPU load is missing because apps can't read `/proc/stat`.

`ThermalState::level()` and `from_level()` convert between nominal/fair/serious/critical and the
0-3 value a reading stores, so phones and desktops share one series format.

### Live while it runs

So, a results page after the fact is nice, but you also want to *watch* the thing heat up. Every
run streams this:

```text
RunEvent::RunStarted { sensors }       once: what this device has, and what it doesn't
RunEvent::Sensors { t, readings }      every second, straight from the sampler
RunEvent::ModuleProgress { progress }  progress.live = the latest number, e.g. 44.3 tok/s
```

The sampler thread needs a `'static` callback while the run's `on_event` is borrowed, so samples
go through a channel to a scoped forwarder thread, which closes when the sampler stops. The same
callback keeps a `SensorSnapshot` with the latest readings, which modules get as `ctx.sensors`.
That way a battery test reads the same numbers the charts show, on a laptop today and on a
phone's providers later. Each `ModuleRecord` also stores `offset_ms`, which lines a module up
with the run's telemetry. In the app, charts have one series and one y-axis each, a sensor that
exists but can't be read says so instead of drawing zeros, and the standard sensors a device
lacks are listed under the charts.

## Before a run

So, first of all, a benchmark is only as fair as the moment you press Start. `preflight::check`
looks at the things that quietly drag results down:

| Check | Level | When |
|---|---|---|
| `low-memory` | warning | less than 25% or 2 GB of memory free |
| `busy-cpu` | warning | other apps using 25% or more of the CPU |
| `battery-saver` | warning | Windows battery saver, macOS Low Power Mode, Linux `low-power` profile |
| `hot-cpu`, `hot-gpu`, `hot-device` | warning | 80 °C or more, or a phone's thermal state at "serious" |
| `low-battery` | warning | under 30% while on battery |
| `on-battery` | info | on battery, and the profile has no battery test |
| `plugged-in` | info | the profile has a battery test, but the charger is in |

The app shows them under Start and refreshes them every 15 seconds, so closing an app clears
the warning. The runner checks again as the run begins, sends them in `RunStarted`, and saves
them in `RunResult.preflight`. A slow number in an old result then comes with its reason
("only 1.0 GB of 15.8 GB free"). Nothing blocks the run: you can still press Start, but you'll
know why the number looks off.

## Burst, sustained and battery

Here's the thing about a quick benchmark: it catches your device at its coolest. That's the
number on the box, not the one you get 20 minutes into a long chat. So InferMark measures both,
the same way phone stress tests do.

- **Burst** is `inference-perf`: a warm-up, then a few short runs, and the median.
- **Sustained** is `sustained-load`: one model generating back to back for `minutes` (5 in
  Standard, 20 in Full). From that we get:
  - **peak**: the best single generation
  - **sustained**: the median of the last quarter, so one lucky or unlucky run can't swing it
  - **stability** = sustained ÷ peak; **throttling** is the rest

  The result also carries `burst_tps` from earlier in the run, so you can see the short-test
  number and the hot number side by side.
- **Battery** is `battery-drain`. It only runs on battery power (the planner skips it with
  "unplug the charger" otherwise) and reads charge and drain from `ctx.sensors` after every
  generation. It reports:
  - charge used and % per 10k tokens
  - average watts and tokens per Wh, where the device reports watts
  - hours of nonstop generation from a full battery: battery capacity ÷ watts when both are
    known, otherwise worked out from the charge drop

  If the charge goes up, or the drain disappears for three generations in a row, the charger
  went back in, so the test fails instead of reporting a number that isn't real. A drop under
  0.5 % with no watt readings is too coarse to trust, and the result says to run it for longer.

The model-running pieces are shared in `crates/core/src/modules/model_run.rs`: choosing the
backend, the hardware gate, the cache-busting prompt, and `generation_loop`. A new
model-based test reuses them, so every test runs the same models the same way.
