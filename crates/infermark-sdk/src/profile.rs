//! Run profiles: the no-code way to decide what InferMark runs and how.
//!
//! A profile is a TOML file. Every field has a sensible default, so a minimal profile is just:
//!
//! ```toml
//! schema = 1
//! name = "My profile"
//!
//! [modules.system-probe]
//! [modules.memory-bandwidth]
//! size_mb = 128
//! ```

use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    /// File format version; must be 1.
    pub schema: u32,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub run: RunSettings,
    #[serde(default)]
    pub gate: GateSettings,
    #[serde(default)]
    pub scoring: ScoringWeights,
    #[serde(default)]
    pub models: ModelSelection,
    #[serde(default)]
    pub packs: PackSelection,
    /// Modules to run, in order. Keys are module ids; values hold `enabled` plus module settings.
    #[serde(default)]
    pub modules: IndexMap<String, ModuleSelection>,
}

impl Profile {
    /// Parse a profile from TOML and check its values.
    pub fn from_toml(text: &str) -> Result<Self, ProfileError> {
        let profile: Profile =
            toml::from_str(text).map_err(|e| ProfileError::Parse(e.to_string()))?;
        profile.validate()?;
        Ok(profile)
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).expect("profile is always serializable")
    }

    pub fn validate(&self) -> Result<(), ProfileError> {
        let invalid = |field: &str, msg: &str| {
            Err(ProfileError::Invalid {
                field: field.to_string(),
                message: msg.to_string(),
            })
        };
        if self.schema != crate::SCHEMA_VERSION {
            return invalid("schema", "unsupported schema version (expected 1)");
        }
        if self.name.trim().is_empty() {
            return invalid("name", "must not be empty");
        }
        if self.run.repeats == 0 {
            return invalid("run.repeats", "must be at least 1");
        }
        if !(self.gate.memory_margin > 0.0 && self.gate.memory_margin <= 1.0) {
            return invalid("gate.memory_margin", "must be between 0 (exclusive) and 1");
        }
        if self.gate.min_tokens_per_sec < 0.0 {
            return invalid("gate.min_tokens_per_sec", "must not be negative");
        }
        let w = &self.scoring;
        let weights = [
            w.speed,
            w.responsiveness,
            w.endurance,
            w.efficiency,
            w.quality,
        ];
        if weights.iter().any(|v| *v < 0.0) {
            return invalid("scoring", "weights must not be negative");
        }
        if weights.iter().sum::<f64>() <= 0.0 {
            return invalid("scoring", "at least one weight must be positive");
        }
        for id in self.modules.keys() {
            if !is_kebab_id(id) {
                return invalid(
                    &format!("modules.{id}"),
                    "module ids are lowercase kebab-case",
                );
            }
        }
        Ok(())
    }

    /// Ids of modules that are enabled, in profile order.
    pub fn enabled_modules(&self) -> impl Iterator<Item = (&String, &ModuleSelection)> {
        self.modules.iter().filter(|(_, m)| m.enabled)
    }
}

/// Ids of modules, packs and profiles are lowercase kebab-case (`memory-bandwidth`).
pub fn is_kebab_id(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with('-')
        && !id.ends_with('-')
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ProfileError {
    #[error("could not parse profile: {0}")]
    Parse(String),
    #[error("invalid `{field}`: {message}")]
    Invalid { field: String, message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RunSettings {
    /// Measured repetitions per test (results report the median).
    #[serde(default = "d_repeats")]
    pub repeats: u32,
    /// Unmeasured warm-up runs before measuring.
    #[serde(default = "d_warmups")]
    pub warmups: u32,
    #[serde(default = "d_seed")]
    pub seed: u64,
    /// CPU threads for inference; `None` = let the backend decide.
    #[serde(default)]
    pub threads: Option<u32>,
    /// `auto`, `llama-cpp`, `ollama`, …
    #[serde(default = "d_backend")]
    pub backend: String,
    /// Layers to offload to the GPU; `None` = automatic, 0 = CPU only.
    #[serde(default)]
    pub gpu_layers: Option<i32>,
    /// Prompt lengths (tokens) used by context-scaling tests.
    #[serde(default = "d_context_sizes")]
    pub context_sizes: Vec<u32>,
    /// Allow reasoning ("thinking") output for models that support it.
    #[serde(default)]
    pub think: bool,
}

impl Default for RunSettings {
    fn default() -> Self {
        Self {
            repeats: d_repeats(),
            warmups: d_warmups(),
            seed: d_seed(),
            threads: None,
            backend: d_backend(),
            gpu_layers: None,
            context_sizes: d_context_sizes(),
            think: false,
        }
    }
}

/// Thresholds that decide which models are too big or too slow for this device.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GateSettings {
    /// Skip models predicted to generate slower than this.
    #[serde(default = "d_min_tps")]
    pub min_tokens_per_sec: f64,
    /// Fraction of available memory a model may use.
    #[serde(default = "d_margin")]
    pub memory_margin: f64,
    /// Run models even when the gate says they won't fit ("I know what I'm doing").
    #[serde(default)]
    pub allow_ineligible: bool,
}

impl Default for GateSettings {
    fn default() -> Self {
        Self {
            min_tokens_per_sec: d_min_tps(),
            memory_margin: d_margin(),
            allow_ineligible: false,
        }
    }
}

/// Relative weights of the sub-scores in the overall InferMark Score.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ScoringWeights {
    #[serde(default = "d_w_speed")]
    pub speed: f64,
    #[serde(default = "d_w_resp")]
    pub responsiveness: f64,
    #[serde(default = "d_w_endurance")]
    pub endurance: f64,
    #[serde(default = "d_w_eff")]
    pub efficiency: f64,
    #[serde(default = "d_w_quality")]
    pub quality: f64,
}

impl Default for ScoringWeights {
    fn default() -> Self {
        Self {
            speed: d_w_speed(),
            responsiveness: d_w_resp(),
            endurance: d_w_endurance(),
            efficiency: d_w_eff(),
            quality: d_w_quality(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ModelSelection {
    /// Catalog model ids; empty = let the gate pick from the whole catalog.
    #[serde(default)]
    pub catalog: Vec<String>,
    /// Ignore catalog models larger than this many billion parameters.
    #[serde(default)]
    pub max_params_b: Option<f64>,
    /// Extra local `.gguf` files to benchmark.
    #[serde(default)]
    pub local_paths: Vec<String>,
    /// Models already installed in Ollama, e.g. `llama3.1:8b`.
    #[serde(default)]
    pub ollama: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PackSelection {
    /// Pack ids to run; `["*"]` = every installed pack.
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    /// Use at most this many items per pack (handy for quick runs).
    #[serde(default)]
    pub max_items_per_pack: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct ModuleSelection {
    #[serde(default = "d_true")]
    pub enabled: bool,
    /// Any other keys are passed to the module as its settings.
    #[serde(flatten)]
    #[schemars(with = "std::collections::BTreeMap<String, serde_json::Value>")]
    pub settings: toml::Table,
}

impl Default for ModuleSelection {
    fn default() -> Self {
        Self {
            enabled: true,
            settings: toml::Table::new(),
        }
    }
}

fn d_repeats() -> u32 {
    3
}
fn d_warmups() -> u32 {
    1
}
fn d_seed() -> u64 {
    42
}
fn d_backend() -> String {
    "auto".into()
}
fn d_context_sizes() -> Vec<u32> {
    vec![512, 2048]
}
fn d_min_tps() -> f64 {
    2.0
}
fn d_margin() -> f64 {
    0.85
}
fn d_w_speed() -> f64 {
    0.25
}
fn d_w_resp() -> f64 {
    0.20
}
fn d_w_endurance() -> f64 {
    0.15
}
fn d_w_eff() -> f64 {
    0.10
}
fn d_w_quality() -> f64 {
    0.30
}
fn d_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_profile_gets_defaults() {
        let p = Profile::from_toml("schema = 1\nname = \"Mini\"\n").unwrap();
        assert_eq!(p.run.repeats, 3);
        assert_eq!(p.gate.memory_margin, 0.85);
        assert!(p.modules.is_empty());
    }

    #[test]
    fn module_settings_are_kept_and_order_preserved() {
        let p = Profile::from_toml(
            r#"
schema = 1
name = "Ordered"
[modules.zeta]
[modules.alpha]
enabled = false
size_mb = 64
"#,
        )
        .unwrap();
        let ids: Vec<_> = p.modules.keys().cloned().collect();
        assert_eq!(ids, ["zeta", "alpha"]);
        let alpha = &p.modules["alpha"];
        assert!(!alpha.enabled);
        assert_eq!(alpha.settings["size_mb"].as_integer(), Some(64));
        assert!(!alpha.settings.contains_key("enabled"));
        assert_eq!(p.enabled_modules().count(), 1);
    }

    #[test]
    fn rejects_unknown_top_level_keys() {
        let err = Profile::from_toml("schema = 1\nname = \"x\"\nrepeats = 2\n").unwrap_err();
        assert!(matches!(err, ProfileError::Parse(_)));
    }

    #[test]
    fn rejects_bad_values() {
        let bad = [
            ("schema = 2\nname = \"x\"", "schema"),
            ("schema = 1\nname = \" \"", "name"),
            (
                "schema = 1\nname = \"x\"\n[run]\nrepeats = 0",
                "run.repeats",
            ),
            (
                "schema = 1\nname = \"x\"\n[gate]\nmemory_margin = 1.5",
                "gate.memory_margin",
            ),
            (
                "schema = 1\nname = \"x\"\n[scoring]\nspeed = -1.0",
                "scoring",
            ),
            (
                "schema = 1\nname = \"x\"\n[modules.Bad_Id]",
                "modules.Bad_Id",
            ),
        ];
        for (text, field) in bad {
            match Profile::from_toml(text) {
                Err(ProfileError::Invalid { field: f, .. }) => assert_eq!(f, field, "{text}"),
                other => panic!("expected invalid {field}, got {other:?}"),
            }
        }
    }

    #[test]
    fn round_trips_through_toml() {
        let mut p = Profile::from_toml("schema = 1\nname = \"Round\"\n").unwrap();
        p.modules
            .insert("system-probe".into(), ModuleSelection::default());
        let again = Profile::from_toml(&p.to_toml()).unwrap();
        assert_eq!(p, again);
    }
}
