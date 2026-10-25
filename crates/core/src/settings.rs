//! App settings (`~/.infermark/settings.toml`), changed on the app's Settings page or with
//! `infermark settings set <key> <value>`.

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use infermark_engine::EngineConfig;
use serde::{Deserialize, Serialize};

use crate::paths;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// Where downloaded models and inference engines are kept (they can be large).
    /// Default: the InferMark folder in your home directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_dir: Option<PathBuf>,
    /// Use this `llama-server` instead of the one InferMark installs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llama_server: Option<PathBuf>,
    #[serde(default = "d_ollama_url")]
    pub ollama_url: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            storage_dir: None,
            llama_server: None,
            ollama_url: d_ollama_url(),
        }
    }
}

fn d_ollama_url() -> String {
    infermark_engine::ollama::DEFAULT_URL.to_string()
}

/// Keys accepted by [`Settings::set`].
pub const KEYS: &[&str] = &["storage_dir", "llama_server", "ollama_url"];

impl Settings {
    pub fn validate(&self) -> Result<()> {
        if let Some(dir) = &self.storage_dir {
            if !dir.is_absolute() {
                bail!("storage_dir must be an absolute path");
            }
        }
        if let Some(exe) = &self.llama_server {
            if !exe.is_file() {
                bail!("llama_server: {} does not exist", exe.display());
            }
        }
        let url = url::Url::parse(&self.ollama_url).context("ollama_url is not a valid URL")?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            bail!(
                "ollama_url must be an http(s) address such as {}",
                d_ollama_url()
            );
        }
        Ok(())
    }

    /// Set one key from text (an empty value resets it to the default). Nothing changes if
    /// the new value is invalid.
    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        let value = value.trim();
        let path = || (!value.is_empty()).then(|| PathBuf::from(value));
        let mut next = self.clone();
        match key {
            "storage_dir" => next.storage_dir = path(),
            "llama_server" => next.llama_server = path(),
            "ollama_url" => {
                next.ollama_url = if value.is_empty() {
                    d_ollama_url()
                } else {
                    value.trim_end_matches('/').to_string()
                }
            }
            other => bail!(
                "unknown setting `{other}` (expected one of: {})",
                KEYS.join(", ")
            ),
        }
        next.validate()?;
        *self = next;
        Ok(())
    }

    /// Folder holding `models/` and `engines/`. `INFERMARK_STORAGE_DIR` overrides the setting.
    pub fn storage_dir(&self) -> PathBuf {
        std::env::var_os("INFERMARK_STORAGE_DIR")
            .map(PathBuf::from)
            .or_else(|| self.storage_dir.clone())
            .unwrap_or_else(paths::home)
    }

    pub fn models_dir(&self) -> PathBuf {
        self.storage_dir().join("models")
    }

    pub fn engines_dir(&self) -> PathBuf {
        self.storage_dir().join("engines")
    }

    pub fn engine_config(&self) -> EngineConfig {
        EngineConfig {
            llama_server: self.llama_server.clone(),
            engines_dir: self.engines_dir(),
            ollama_url: self.ollama_url.clone(),
        }
    }
}

pub fn path() -> PathBuf {
    paths::home().join("settings.toml")
}

/// The saved settings, or defaults when there is no file yet.
pub fn load() -> Result<Settings> {
    let p = path();
    match std::fs::read_to_string(&p) {
        Ok(text) => {
            let s: Settings =
                toml::from_str(&text).with_context(|| format!("could not read {}", p.display()))?;
            Ok(s)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(e) => Err(e).with_context(|| format!("could not read {}", p.display())),
    }
}

/// Like [`load`], but falls back to defaults if the file is broken (so a run never fails on it).
pub fn load_or_default() -> Settings {
    load().unwrap_or_default()
}

pub fn save(settings: &Settings) -> Result<()> {
    settings.validate()?;
    let p = path();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = toml::to_string_pretty(settings).expect("settings serialize");
    std::fs::write(&p, text).with_context(|| format!("could not write {}", p.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_set() {
        let mut s = Settings::default();
        assert_eq!(s.ollama_url, "http://127.0.0.1:11434");
        s.set("ollama_url", "http://localhost:11434/").unwrap();
        assert_eq!(s.ollama_url, "http://localhost:11434");
        assert!(s.set("ollama_url", "ftp://x").is_err());
        assert!(s.set("storage_dir", "relative/path").is_err());
        let abs = std::env::temp_dir();
        s.set("storage_dir", abs.to_str().unwrap()).unwrap();
        assert_eq!(s.models_dir(), abs.join("models"));
        s.set("storage_dir", "").unwrap();
        assert_eq!(s.storage_dir, None);
        assert!(s.set("nope", "1").is_err());
    }

    #[test]
    fn round_trips_and_rejects_unknown_keys() {
        let s = Settings {
            storage_dir: Some(std::env::temp_dir()),
            ..Default::default()
        };
        let text = toml::to_string_pretty(&s).unwrap();
        assert_eq!(toml::from_str::<Settings>(&text).unwrap(), s);
        assert!(toml::from_str::<Settings>("telemetry = true").is_err());
    }
}
