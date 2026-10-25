//! Where InferMark looks for user content.
//!
//! ```text
//! ~/.infermark/
//!   profiles/   your own *.toml profiles
//!   packs/      installed test packs (one folder each)
//!   results/    result JSON files
//! ```
//!
//! `INFERMARK_HOME` overrides `~/.infermark`; `INFERMARK_PACKS_DIR` adds an extra pack folder.

use std::path::PathBuf;

pub fn home() -> PathBuf {
    if let Some(dir) = std::env::var_os("INFERMARK_HOME") {
        return PathBuf::from(dir);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".infermark")
}

pub fn user_profiles_dir() -> PathBuf {
    home().join("profiles")
}

pub fn user_packs_dir() -> PathBuf {
    home().join("packs")
}

pub fn results_dir() -> PathBuf {
    home().join("results")
}

/// Folders searched for packs, in priority order (later folders cannot shadow earlier ids).
pub fn pack_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(extra) = std::env::var_os("INFERMARK_PACKS_DIR") {
        dirs.push(PathBuf::from(extra));
    }
    dirs.push(user_packs_dir());
    // Packs shipped next to the executable (installed app bundles them here).
    if let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from))
    {
        dirs.push(exe_dir.join("packs"));
    }
    // Packs in the source tree, for development builds.
    dirs.push(PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../packs"
    )));
    dirs
}
