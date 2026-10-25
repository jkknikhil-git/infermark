//! Scorers grade a model's answer to a pack item.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::pack::{PackItem, PackManifest};

pub trait Scorer: Send + Sync {
    /// Id referenced by `pack.toml` (`scorer = "exact_match"`).
    fn id(&self) -> &'static str;

    /// Check an item has what this scorer needs (e.g. `expected`). Called when a pack is loaded.
    fn validate_item(&self, item: &PackItem) -> Result<(), String>;

    fn score(&self, pack: &PackManifest, item: &PackItem, answer: &str) -> ScoreResult;
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct ScoreResult {
    /// 0.0 (wrong) to 1.0 (perfect).
    pub score: f64,
    pub passed: bool,
    #[serde(default)]
    pub explanation: Option<String>,
}

impl ScoreResult {
    pub fn pass() -> Self {
        Self {
            score: 1.0,
            passed: true,
            explanation: None,
        }
    }

    pub fn fail(why: impl Into<String>) -> Self {
        Self {
            score: 0.0,
            passed: false,
            explanation: Some(why.into()),
        }
    }
}
