use infermark_sdk::{HardwareInfo, PlanEntry, PlanStatus, Profile};

use crate::Registry;

/// Decide what will run. Every enabled module in the profile gets an entry, in profile order:
/// either `Planned`, or `Skipped` with a reason the user can read.
pub fn plan(profile: &Profile, registry: &Registry, hardware: &HardwareInfo) -> Vec<PlanEntry> {
    profile
        .enabled_modules()
        .map(|(id, _)| match registry.module(id) {
            None => PlanEntry {
                module_id: id.clone(),
                descriptor: None,
                status: PlanStatus::Skipped {
                    reason: "not available in this build of InferMark".into(),
                },
            },
            Some(module) => {
                let descriptor = module.descriptor();
                let unmet: Vec<String> = descriptor
                    .requirements
                    .iter()
                    .filter_map(|r| r.check(hardware).err())
                    .collect();
                let status = if unmet.is_empty() {
                    PlanStatus::Planned
                } else {
                    PlanStatus::Skipped {
                        reason: unmet.join("; "),
                    }
                };
                PlanEntry {
                    module_id: id.clone(),
                    descriptor: Some(descriptor),
                    status,
                }
            }
        })
        .collect()
}
