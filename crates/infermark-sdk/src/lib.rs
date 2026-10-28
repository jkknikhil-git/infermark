//! # InferMark SDK
//!
//! The stable surface that every InferMark module builds on. There are five extension points:
//!
//! | Trait | What it adds |
//! | --- | --- |
//! | [`BenchModule`] | A benchmark that can be planned and run (perf, thermal, battery, quality…) |
//! | [`Backend`] | An inference engine (llama.cpp, Ollama, ONNX Runtime…) |
//! | [`SensorProvider`] | A source of telemetry (temperatures, clocks, power, battery) |
//! | [`Scorer`] | A way to grade a model answer against a pack item |
//! | [`Exporter`] | A result output format (JSON, CSV, HTML…) |
//!
//! Modules declare [`Requirement`]s; the planner skips any module whose requirements the
//! detected [`HardwareInfo`] does not meet, and records why.

pub mod backend;
pub mod exporter;
pub mod hardware;
pub mod module;
pub mod pack;
pub mod profile;
pub mod result;
pub mod scorer;
pub mod sensor;

pub use backend::{Backend, GenerateRequest, GenerationStats, LoadOptions, ModelSpec, TokenEvent};
pub use exporter::Exporter;
pub use hardware::{
    BatteryInfo, CpuInfo, GpuInfo, GpuVendor, HardwareInfo, MemoryInfo, NpuInfo, OsFamily, OsInfo,
    PowerSource,
};
pub use module::{
    BenchModule, Direction, LiveValue, Metric, ModuleCategory, ModuleDescriptor, ModuleError,
    ModuleOutput, Progress, Requirement, RunContext, Series,
};
pub use pack::{Expected, GenerationSettings, PackItem, PackManifest, PromptTemplate};
pub use profile::{
    is_kebab_id, GateSettings, ModelSelection, ModuleSelection, PackSelection, Profile,
    ProfileError, RunSettings, ScoringWeights,
};
pub use result::{ModuleRecord, ModuleStatus, PlanEntry, PlanStatus, RunResult, Telemetry};
pub use scorer::{ScoreResult, Scorer};
pub use sensor::{
    SensorChannel, SensorInventory, SensorKind, SensorProvider, SensorReading, SensorSnapshot,
    ThermalState, STANDARD_CHANNELS,
};

/// Version of the result / profile / pack file formats produced by this SDK.
pub const SCHEMA_VERSION: u32 = 1;
