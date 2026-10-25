//! Built-in and user profiles.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use infermark_sdk::Profile;
use serde::Serialize;

/// Profiles compiled into every build, so the CLI and app work without any files on disk.
const BUILTIN: &[(&str, &str)] = &[
    ("quick", include_str!("../../../profiles/quick.toml")),
    ("standard", include_str!("../../../profiles/standard.toml")),
    ("full", include_str!("../../../profiles/full.toml")),
    (
        "laptop-battery",
        include_str!("../../../profiles/laptop-battery.toml"),
    ),
];

#[derive(Debug, Clone, Serialize)]
pub struct ProfileEntry {
    /// Name used to select it (`--profile quick`).
    pub key: String,
    pub source: ProfileSource,
    pub profile: Profile,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "path", rename_all = "kebab-case")]
pub enum ProfileSource {
    Builtin,
    User(PathBuf),
}

/// All available profiles: built-ins first, then `~/.infermark/profiles/*.toml`.
/// A user profile with the same key as a built-in replaces it. Invalid user files are returned
/// as errors alongside the valid profiles so the UI can show them.
pub fn list() -> (Vec<ProfileEntry>, Vec<(PathBuf, String)>) {
    let mut entries: Vec<ProfileEntry> = BUILTIN
        .iter()
        .map(|(key, text)| ProfileEntry {
            key: key.to_string(),
            source: ProfileSource::Builtin,
            profile: Profile::from_toml(text).expect("built-in profiles are valid"),
        })
        .collect();
    let mut errors = Vec::new();

    let dir = crate::paths::user_profiles_dir();
    if let Ok(read) = std::fs::read_dir(&dir) {
        let mut paths: Vec<PathBuf> = read
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "toml"))
            .collect();
        paths.sort();
        for path in paths {
            let key = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            match load_file(&path) {
                Ok(profile) => {
                    entries.retain(|e| e.key != key);
                    entries.push(ProfileEntry {
                        key,
                        source: ProfileSource::User(path),
                        profile,
                    });
                }
                Err(e) => errors.push((path, format!("{e:#}"))),
            }
        }
    }
    (entries, errors)
}

/// Resolve `--profile` input: a profile key (`quick`) or a path to a TOML file.
pub fn resolve(name_or_path: &str) -> Result<Profile> {
    let path = Path::new(name_or_path);
    if path.extension().is_some_and(|e| e == "toml") || path.is_file() {
        return load_file(path);
    }
    let (entries, _) = list();
    match entries.into_iter().find(|e| e.key == name_or_path) {
        Some(e) => Ok(e.profile),
        None => bail!("no profile named `{name_or_path}` (try `infermark profiles`)"),
    }
}

pub fn load_file(path: &Path) -> Result<Profile> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    Profile::from_toml(&text).with_context(|| format!("in {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_profiles_are_valid() {
        for (key, text) in BUILTIN {
            let p = Profile::from_toml(text).unwrap_or_else(|e| panic!("{key}: {e}"));
            assert!(p.enabled_modules().count() > 0, "{key} has no modules");
        }
    }

    #[test]
    fn resolve_by_key_and_path() {
        assert_eq!(resolve("quick").unwrap().name, "Quick");
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("mine.toml");
        std::fs::write(&file, "schema = 1\nname = \"Mine\"\n").unwrap();
        assert_eq!(resolve(file.to_str().unwrap()).unwrap().name, "Mine");
        assert!(resolve("does-not-exist").is_err());
    }
}
