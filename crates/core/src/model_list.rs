//! The catalog as seen from this device: installed or not, and the hardware gate's verdict.
//! Shared by `infermark models` and the app's Models page.

use infermark_models::{assess, Catalog, CatalogModel, ModelStore, Verdict};
use infermark_sdk::{GateSettings, HardwareInfo, ModuleStatus, RunResult};
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
    bandwidth_gbps: Option<f64>,
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
                bandwidth_gbps,
            ),
            model,
        })
        .collect()
}

/// Memory bandwidth (triad, GB/s) from the newest saved result for this hardware, so the gate
/// can predict speed without re-measuring.
pub fn latest_bandwidth(fingerprint: &str) -> Option<f64> {
    let mut files: Vec<_> = std::fs::read_dir(paths::results_dir())
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    // Result files start with a timestamp, so the newest sorts last.
    files.sort();
    files.iter().rev().take(50).find_map(|p| {
        let text = std::fs::read_to_string(p).ok()?;
        let r: RunResult = serde_json::from_str(&text).ok()?;
        if r.hardware.fingerprint != fingerprint {
            return None;
        }
        r.modules
            .iter()
            .filter(|m| m.module_id == "memory-bandwidth" && m.status == ModuleStatus::Completed)
            .flat_map(|m| &m.output.metrics)
            .find(|m| m.key == "triad_gbps")
            .map(|m| m.value)
    })
}
