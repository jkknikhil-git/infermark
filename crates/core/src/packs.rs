//! Loading and validating test packs.
//!
//! A pack is a folder:
//!
//! ```text
//! my-pack/
//!   pack.toml     manifest (see `PackManifest`)
//!   items.jsonl   one `PackItem` per line
//! ```

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use infermark_sdk::{is_kebab_id, PackItem, PackManifest, SCHEMA_VERSION};
use serde::Serialize;

use crate::Registry;

#[derive(Debug, Clone, Serialize)]
pub struct LoadedPack {
    pub dir: PathBuf,
    pub manifest: PackManifest,
    pub items: Vec<PackItem>,
}

/// Everything wrong with a pack, so authors can fix all problems in one go.
#[derive(Debug, Clone, Serialize, thiserror::Error)]
#[error("pack at {dir} has {} problem(s):\n  - {}", .issues.len(), .issues.join("\n  - "))]
pub struct PackError {
    pub dir: PathBuf,
    pub issues: Vec<String>,
}

pub fn load(dir: &Path, registry: &Registry) -> Result<LoadedPack, PackError> {
    let fail = |issues: Vec<String>| PackError {
        dir: dir.to_path_buf(),
        issues,
    };

    let manifest_text = std::fs::read_to_string(dir.join("pack.toml"))
        .map_err(|e| fail(vec![format!("cannot read pack.toml: {e}")]))?;
    let manifest: PackManifest =
        toml::from_str(&manifest_text).map_err(|e| fail(vec![format!("pack.toml: {e}")]))?;

    let mut issues = Vec::new();
    if manifest.schema != SCHEMA_VERSION {
        issues.push(format!(
            "pack.toml: unsupported schema {} (expected 1)",
            manifest.schema
        ));
    }
    if !is_kebab_id(&manifest.id) {
        issues.push(format!(
            "pack.toml: id {:?} must be lowercase kebab-case",
            manifest.id
        ));
    }
    for (field, value) in [
        ("name", &manifest.name),
        ("version", &manifest.version),
        ("category", &manifest.category),
        ("license", &manifest.license),
    ] {
        if value.trim().is_empty() {
            issues.push(format!("pack.toml: `{field}` must not be empty"));
        }
    }
    if !manifest.prompt.template.contains("{{input}}") {
        issues.push("pack.toml: prompt.template must contain {{input}}".into());
    }
    let scorer = registry.scorer(&manifest.scorer);
    if scorer.is_none() {
        let known: Vec<_> = registry.scorer_ids().collect();
        issues.push(format!(
            "pack.toml: unknown scorer {:?} (available: {})",
            manifest.scorer,
            known.join(", ")
        ));
    }

    let items_text = match std::fs::read_to_string(dir.join("items.jsonl")) {
        Ok(t) => t,
        Err(e) => {
            issues.push(format!("cannot read items.jsonl: {e}"));
            return Err(fail(issues));
        }
    };
    let mut items = Vec::new();
    let mut seen = HashSet::new();
    for (n, line) in items_text.lines().enumerate() {
        let line_no = n + 1;
        if line.trim().is_empty() {
            continue;
        }
        let item: PackItem = match serde_json::from_str(line) {
            Ok(i) => i,
            Err(e) => {
                issues.push(format!("items.jsonl line {line_no}: {e}"));
                continue;
            }
        };
        if item.id.trim().is_empty() {
            issues.push(format!("items.jsonl line {line_no}: empty id"));
        } else if !seen.insert(item.id.clone()) {
            issues.push(format!(
                "items.jsonl line {line_no}: duplicate id {:?}",
                item.id
            ));
        }
        if let Some(scorer) = scorer {
            if let Err(e) = scorer.validate_item(&item) {
                issues.push(format!("items.jsonl line {line_no} ({}): {e}", item.id));
            }
        }
        items.push(item);
    }
    if items.is_empty() && issues.is_empty() {
        issues.push("items.jsonl has no items".into());
    }

    if issues.is_empty() {
        Ok(LoadedPack {
            dir: dir.to_path_buf(),
            manifest,
            items,
        })
    } else {
        Err(fail(issues))
    }
}

/// Find every pack under the given folders. The first pack with a given id wins.
pub fn discover(search_dirs: &[PathBuf], registry: &Registry) -> (Vec<LoadedPack>, Vec<PackError>) {
    let mut packs: Vec<LoadedPack> = Vec::new();
    let mut errors = Vec::new();
    for root in search_dirs {
        let Ok(read) = std::fs::read_dir(root) else {
            continue;
        };
        let mut dirs: Vec<PathBuf> = read
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.join("pack.toml").is_file())
            .collect();
        dirs.sort();
        for dir in dirs {
            match load(&dir, registry) {
                Ok(pack) if packs.iter().any(|p| p.manifest.id == pack.manifest.id) => {}
                Ok(pack) => packs.push(pack),
                Err(e) => errors.push(e),
            }
        }
    }
    (packs, errors)
}

/// Apply a profile's pack selection.
pub fn select<'a>(
    packs: &'a [LoadedPack],
    selection: &infermark_sdk::PackSelection,
) -> Vec<&'a LoadedPack> {
    let all = selection.include.iter().any(|i| i == "*");
    packs
        .iter()
        .filter(|p| all || selection.include.contains(&p.manifest.id))
        .filter(|p| !selection.exclude.contains(&p.manifest.id))
        .collect()
}
