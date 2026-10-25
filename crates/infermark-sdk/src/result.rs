//! The result file written after every run (`result.json`, schema v1).

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::hardware::HardwareInfo;
use crate::module::{ModuleDescriptor, ModuleOutput};
use crate::profile::Profile;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct RunResult {
    pub schema: u32,
    pub infermark_version: String,
    pub run_id: Uuid,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub hardware: HardwareInfo,
    /// The exact profile used, so the run can be reproduced.
    pub profile: Profile,
    pub plan: Vec<PlanEntry>,
    pub modules: Vec<ModuleRecord>,
}

/// What the planner decided for one module, before anything ran.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct PlanEntry {
    pub module_id: String,
    /// `None` when the profile names a module this build doesn't have.
    pub descriptor: Option<ModuleDescriptor>,
    pub status: PlanStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum PlanStatus {
    Planned,
    Skipped { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct ModuleRecord {
    pub module_id: String,
    pub status: ModuleStatus,
    pub duration_ms: f64,
    #[serde(default)]
    pub output: ModuleOutput,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum ModuleStatus {
    Completed,
    Skipped { reason: String },
    Failed { error: String },
    Cancelled,
}
