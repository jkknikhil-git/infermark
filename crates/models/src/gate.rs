//! The hardware gate: does a model fit this device, and how fast is it likely to be?
//!
//! ```text
//! needed  = (weights + KV cache for the context) × 1.15 overhead
//! budget  = GPU memory × margin  (+ system RAM × margin when layers can be offloaded)
//! speed   ≈ memory bandwidth × 0.6 ÷ bytes read per token
//! ```
//!
//! Token generation reads every active weight once per token, so memory bandwidth is the best
//! single predictor of tokens/sec. Predictions are deliberately rough; they only decide what is
//! worth running, never what a result says.

use infermark_sdk::{GateSettings, HardwareInfo};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
/// Runtime buffers, compute graph and fragmentation on top of weights + KV cache.
pub const OVERHEAD: f64 = 0.15;
/// Share of measured (STREAM) bandwidth that token generation reaches in practice.
pub const BANDWIDTH_EFFICIENCY: f64 = 0.6;

/// The architecture numbers that decide memory use.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ModelShape {
    pub layers: u32,
    /// Key/value heads (fewer than attention heads with grouped-query attention).
    pub kv_heads: u32,
    pub head_dim: u32,
    /// Longest context the model was trained for.
    pub context_length: u32,
    /// For mixture-of-experts models: share of weights read per token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_fraction: Option<f64>,
}

impl ModelShape {
    /// f16 KV cache for `context` tokens: K and V × layers × tokens × kv_heads × head_dim × 2 bytes.
    pub fn kv_cache_bytes(&self, context: u32) -> u64 {
        let tokens = context.min(self.context_length) as u64;
        2 * self.layers as u64 * tokens * self.kv_heads as u64 * self.head_dim as u64 * 2
    }
}

/// Memory a model needs at a given context length.
pub fn memory_needed(file_bytes: u64, shape: &ModelShape, context: u32) -> u64 {
    ((file_bytes + shape.kv_cache_bytes(context)) as f64 * (1.0 + OVERHEAD)) as u64
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Tier {
    /// Everything fits in dedicated GPU memory.
    FullGpu,
    /// Some layers on the GPU, the rest in system memory.
    PartialOffload,
    /// Runs from system memory (CPU, or an integrated GPU sharing RAM).
    SystemMemory,
    /// Needs more memory than the device has.
    TooBig,
    /// Fits, but predicted to be slower than the profile's minimum.
    TooSlow,
}

impl Tier {
    pub fn eligible(self) -> bool {
        matches!(
            self,
            Tier::FullGpu | Tier::PartialOffload | Tier::SystemMemory
        )
    }

    pub fn label(self) -> &'static str {
        match self {
            Tier::FullGpu => "Full GPU",
            Tier::PartialOffload => "Partial offload",
            Tier::SystemMemory => "System memory",
            Tier::TooBig => "Too big",
            Tier::TooSlow => "Too slow",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct Verdict {
    pub tier: Tier,
    pub needed_bytes: u64,
    /// Memory the model may use here (GPU and/or system, after the safety margin).
    pub budget_bytes: u64,
    /// `None` when there is no bandwidth measurement for where the model would run.
    pub predicted_tokens_per_sec: Option<f64>,
    /// One plain-language line for the UI, e.g. "needs 49.6 GB, this device can use 13.4 GB".
    pub summary: String,
}

impl Verdict {
    /// Whether to run the model under these gate settings.
    pub fn runnable(&self, gate: &GateSettings) -> bool {
        self.tier.eligible() || gate.allow_ineligible
    }
}

/// Assess one model. `system_bandwidth_gbps` is the measured memory bandwidth (10^9 bytes/s),
/// e.g. the `triad_gbps` metric from the memory-speed module, if available.
pub fn assess(
    file_bytes: u64,
    shape: &ModelShape,
    context: u32,
    hw: &HardwareInfo,
    gate: &GateSettings,
    system_bandwidth_gbps: Option<f64>,
) -> Verdict {
    let needed = memory_needed(file_bytes, shape, context);
    let ram_budget = (hw.memory.total_bytes as f64 * gate.memory_margin) as u64;
    let vram_budget = (hw.max_dedicated_vram_bytes() as f64 * gate.memory_margin) as u64;
    let active = shape.active_fraction.unwrap_or(1.0).clamp(0.01, 1.0);
    let predict = |bytes_in_ram: u64| {
        system_bandwidth_gbps.filter(|b| *b > 0.0).map(|bw| {
            let per_token = (bytes_in_ram as f64 * active).max(1.0);
            bw * 1e9 * BANDWIDTH_EFFICIENCY / per_token
        })
    };

    let (tier, budget, predicted, summary) = if vram_budget > 0 && needed <= vram_budget {
        (
            Tier::FullGpu,
            vram_budget,
            None,
            format!("Fits in GPU memory ({} of {})", gb(needed), gb(vram_budget)),
        )
    } else if vram_budget > 0 && needed <= vram_budget + ram_budget {
        // Weights that don't fit on the GPU stay in RAM and set the pace.
        let in_ram = file_bytes.saturating_sub(vram_budget).max(file_bytes / 10);
        (
            Tier::PartialOffload,
            vram_budget + ram_budget,
            predict(in_ram),
            format!(
                "Partly on the GPU: {} of {} in GPU memory",
                gb(vram_budget.min(needed)),
                gb(needed)
            ),
        )
    } else if needed <= ram_budget {
        let shared = hw.gpus.iter().any(|g| g.integrated) && vram_budget == 0;
        (
            Tier::SystemMemory,
            ram_budget,
            predict(file_bytes),
            if shared {
                format!(
                    "Runs in system memory, shared with the integrated GPU ({} of {})",
                    gb(needed),
                    gb(ram_budget)
                )
            } else {
                format!(
                    "Runs in system memory ({} of {})",
                    gb(needed),
                    gb(ram_budget)
                )
            },
        )
    } else {
        let budget = vram_budget + ram_budget;
        (
            Tier::TooBig,
            budget,
            None,
            format!("Needs {}, this device can use {}", gb(needed), gb(budget)),
        )
    };

    match predicted {
        Some(tps) if tier.eligible() && tps < gate.min_tokens_per_sec => Verdict {
            tier: Tier::TooSlow,
            needed_bytes: needed,
            budget_bytes: budget,
            predicted_tokens_per_sec: Some(tps),
            summary: format!(
                "Predicted about {tps:.1} tokens/s, below the {} tokens/s minimum",
                gate.min_tokens_per_sec
            ),
        },
        _ => Verdict {
            tier,
            needed_bytes: needed,
            budget_bytes: budget,
            predicted_tokens_per_sec: predicted,
            summary,
        },
    }
}

fn gb(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / GIB)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use infermark_sdk::{CpuInfo, GpuInfo, GpuVendor, MemoryInfo, OsFamily, OsInfo, PowerSource};

    pub(crate) fn device(ram_gb: f64, gpu: Option<(f64, bool)>) -> HardwareInfo {
        let gib = |g: f64| (g * GIB) as u64;
        HardwareInfo {
            fingerprint: "test".into(),
            os: OsInfo {
                family: OsFamily::Windows,
                name: "Windows".into(),
                version: "11".into(),
                kernel: String::new(),
                arch: "x86_64".into(),
            },
            cpu: CpuInfo {
                brand: "Test CPU".into(),
                vendor: String::new(),
                physical_cores: Some(4),
                logical_cores: 8,
                frequency_mhz: None,
            },
            memory: MemoryInfo {
                total_bytes: gib(ram_gb),
                available_bytes: gib(ram_gb / 2.0),
            },
            gpus: gpu
                .map(|(vram, integrated)| GpuInfo {
                    name: "Test GPU".into(),
                    vendor: GpuVendor::Nvidia,
                    dedicated_vram_bytes: Some(gib(vram)),
                    shared_memory_bytes: None,
                    driver_version: None,
                    integrated,
                })
                .into_iter()
                .collect(),
            npus: vec![],
            battery: None,
            power_source: PowerSource::Ac,
        }
    }

    // Llama-3-style shapes.
    const SHAPE_3B: ModelShape = ModelShape {
        layers: 28,
        kv_heads: 8,
        head_dim: 128,
        context_length: 131072,
        active_fraction: None,
    };
    const SHAPE_70B: ModelShape = ModelShape {
        layers: 80,
        kv_heads: 8,
        head_dim: 128,
        context_length: 131072,
        active_fraction: None,
    };
    const Q4_3B: u64 = 2_019_377_376;
    const Q4_70B: u64 = 42_520_395_584;

    #[test]
    fn kv_cache_matches_hand_calculation() {
        // 2 × 28 layers × 4096 tokens × 8 heads × 128 dims × 2 bytes = 448 MiB
        assert_eq!(SHAPE_3B.kv_cache_bytes(4096), 448 * 1024 * 1024);
        // Context is capped at what the model supports.
        let small = ModelShape {
            context_length: 2048,
            ..SHAPE_3B
        };
        assert_eq!(small.kv_cache_bytes(8192), small.kv_cache_bytes(2048));
    }

    #[test]
    fn seventy_b_on_16gb_without_gpu_is_too_big() {
        let v = assess(
            Q4_70B,
            &SHAPE_70B,
            2048,
            &device(16.0, None),
            &GateSettings::default(),
            Some(25.0),
        );
        assert_eq!(v.tier, Tier::TooBig);
        assert!(!v.runnable(&GateSettings::default()));
        assert!(v.summary.starts_with("Needs "), "{}", v.summary);
        let allow = GateSettings {
            allow_ineligible: true,
            ..Default::default()
        };
        assert!(v.runnable(&allow));
    }

    #[test]
    fn three_b_on_16gb_runs_in_system_memory() {
        let v = assess(
            Q4_3B,
            &SHAPE_3B,
            2048,
            &device(16.0, Some((0.125, true))),
            &GateSettings::default(),
            Some(25.0),
        );
        assert_eq!(v.tier, Tier::SystemMemory);
        assert!(v.summary.contains("integrated GPU"));
        // 25 GB/s × 0.6 ÷ 2.0 GB ≈ 7.4 tokens/s
        let tps = v.predicted_tokens_per_sec.unwrap();
        assert!((7.0..8.0).contains(&tps), "{tps}");
    }

    #[test]
    fn dedicated_gpu_tiers() {
        let hw = device(32.0, Some((8.0, false)));
        let gate = GateSettings::default();
        assert_eq!(
            assess(Q4_3B, &SHAPE_3B, 2048, &hw, &gate, None).tier,
            Tier::FullGpu
        );
        let v = assess(9_000_000_000, &SHAPE_70B, 2048, &hw, &gate, Some(50.0));
        assert_eq!(v.tier, Tier::PartialOffload);
        assert!(v.predicted_tokens_per_sec.is_some());
    }

    #[test]
    fn slow_models_are_gated_by_min_speed() {
        let gate = GateSettings {
            min_tokens_per_sec: 5.0,
            ..Default::default()
        };
        let v = assess(
            8_000_000_000,
            &SHAPE_70B,
            2048,
            &device(16.0, None),
            &gate,
            Some(25.0),
        );
        assert_eq!(v.tier, Tier::TooSlow);
        assert!(
            v.summary.contains("below the 5 tokens/s minimum"),
            "{}",
            v.summary
        );
        // Without a bandwidth measurement nothing is gated on speed.
        let v = assess(
            8_000_000_000,
            &SHAPE_70B,
            2048,
            &device(16.0, None),
            &gate,
            None,
        );
        assert_eq!(v.tier, Tier::SystemMemory);
    }

    #[test]
    fn mixture_of_experts_reads_fewer_bytes_per_token() {
        let dense = assess(
            8_000_000_000,
            &SHAPE_3B,
            2048,
            &device(16.0, None),
            &GateSettings::default(),
            Some(25.0),
        );
        let moe_shape = ModelShape {
            active_fraction: Some(0.25),
            ..SHAPE_3B
        };
        let moe = assess(
            8_000_000_000,
            &moe_shape,
            2048,
            &device(16.0, None),
            &GateSettings::default(),
            Some(25.0),
        );
        let ratio = moe.predicted_tokens_per_sec.unwrap() / dense.predicted_tokens_per_sec.unwrap();
        assert!((ratio - 4.0).abs() < 1e-6);
    }
}
