//! The catalog as seen from this device: installed or not, and the hardware gate's verdict.
//! Shared by `infermark models` and the app's Models page.

use infermark_models::{assess, Catalog, CatalogModel, ModelStore, SpeedBasis, Verdict};
use infermark_sdk::{GateSettings, HardwareInfo, ModuleRecord, ModuleStatus, RunResult};
use serde::Serialize;

use crate::paths;

/// Context length the Models page assesses at (prompt + answer of a typical chat).
pub const DEFAULT_CONTEXT: u32 = 4096;

#[derive(Debug, Clone, Serialize)]
pub struct ModelEntry {
    #[serde(flatten)]
    pub model: CatalogModel,
    pub installed: bool,
    /// Bytes of an interrupted download that will be resumed.
    pub partial_bytes: u64,
    pub verdict: Verdict,
}

pub fn list(
    hw: &HardwareInfo,
    gate: &GateSettings,
    speed: Option<SpeedBasis>,
    store: &ModelStore,
) -> Vec<ModelEntry> {
    Catalog::builtin()
        .models
        .into_iter()
        .map(|model| ModelEntry {
            installed: store.is_installed(&model),
            partial_bytes: store.partial_bytes(&model),
            verdict: assess(
                model.size_bytes,
                &model.shape,
                DEFAULT_CONTEXT,
                hw,
                gate,
                speed,
            ),
            model,
        })
        .collect()
}

/// How many recent runs with a measured model count towards a prediction.
const RECENT_RUNS: usize = 5;

/// What to predict model speeds from on this hardware, using its saved results.
///
/// A model this device has really run beats any estimate, unless it has a dedicated GPU: speed
/// there says nothing about models that run from system memory. Recent runs are compared
/// and the best kept, because other apps can only make a run slower. Until then, the newest
/// memory bandwidth.
pub fn speed_basis(hw: &HardwareInfo) -> Option<SpeedBasis> {
    let mut files: Vec<_> = std::fs::read_dir(paths::results_dir())
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    // Result files start with a timestamp, so the newest sorts last.
    files.sort();
    let results: Vec<RunResult> = files
        .iter()
        .rev()
        .take(50)
        .filter_map(|p| serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok())
        .filter(|r: &RunResult| r.hardware.fingerprint == hw.fingerprint)
        .collect();
    let measured = (hw.max_dedicated_vram_bytes() == 0)
        .then(|| {
            let catalog = Catalog::builtin();
            results
                .iter()
                .filter_map(|r| measured_gbps(&r.modules, &catalog))
                .take(RECENT_RUNS)
                .reduce(f64::max)
        })
        .flatten();
    measured.map(SpeedBasis::Measured).or_else(|| {
        results
            .iter()
            .find_map(|r| bandwidth_gbps(&r.modules))
            .map(SpeedBasis::Bandwidth)
    })
}

/// Fastest catalog model in a run, as weights read per second (tokens/sec × file size, in GB/s).
fn measured_gbps(modules: &[ModuleRecord], catalog: &Catalog) -> Option<f64> {
    modules
        .iter()
        .filter(|m| m.module_id == "inference-perf" && m.status == ModuleStatus::Completed)
        .flat_map(|m| &m.output.metrics)
        .filter_map(|m| {
            let model = catalog.get(m.key.strip_suffix("/gen_tps")?)?;
            Some(m.value * model.size_bytes as f64 / 1e9)
        })
        .reduce(f64::max)
}

/// Best of copy and triad in a run, in GB/s.
fn bandwidth_gbps(modules: &[ModuleRecord]) -> Option<f64> {
    modules
        .iter()
        .filter(|m| m.module_id == "memory-bandwidth" && m.status == ModuleStatus::Completed)
        .flat_map(|m| &m.output.metrics)
        .filter(|m| m.key == "copy_gbps" || m.key == "triad_gbps")
        .map(|m| m.value)
        .reduce(f64::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modules(module_id: &str, metrics: &[(&str, f64)]) -> Vec<ModuleRecord> {
        let metrics: Vec<_> = metrics
            .iter()
            .map(|(key, value)| {
                serde_json::json!({
                    "key": key, "label": key, "value": value, "unit": "", "direction": "neutral"
                })
            })
            .collect();
        serde_json::from_value(serde_json::json!([{
            "module_id": module_id,
            "status": {"status": "completed"},
            "duration_ms": 1.0,
            "output": {"metrics": metrics},
        }]))
        .unwrap()
    }

    #[test]
    fn measured_speed_uses_the_model_size_and_ignores_unknown_models() {
        let catalog = Catalog::builtin();
        let size = catalog.get("qwen3-0.6b-q4").unwrap().size_bytes as f64 / 1e9;
        let run = modules(
            "inference-perf",
            &[
                ("qwen3-0.6b-q4/gen_tps", 19.2),
                ("qwen3-0.6b-q4/prompt_tps", 139.0),
                ("my-local-file/gen_tps", 500.0),
            ],
        );
        let gbps = measured_gbps(&run, &catalog).unwrap();
        assert!((gbps - 19.2 * size).abs() < 1e-9);
        // The thread sweep tries settings on purpose, so it doesn't count.
        let sweep = modules("thread-sweep", &[("qwen3-0.6b-q4/gen_tps", 99.0)]);
        assert_eq!(measured_gbps(&sweep, &catalog), None);
    }

    #[test]
    fn bandwidth_is_the_better_of_copy_and_triad() {
        let run = modules(
            "memory-bandwidth",
            &[
                ("copy_gbps", 24.7),
                ("triad_gbps", 27.9),
                ("array_mb", 128.0),
            ],
        );
        assert_eq!(bandwidth_gbps(&run), Some(27.9));
        assert_eq!(bandwidth_gbps(&[]), None);
    }
}
