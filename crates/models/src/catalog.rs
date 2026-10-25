//! The model catalog (`catalog/models.toml`): curated open-weight GGUF models with pinned
//! download URLs, exact sizes and SHA-256 hashes. Data only; never executed.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::gate::ModelShape;

const BUILTIN: &str = include_str!("../../../catalog/models.toml");

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub schema: u32,
    #[serde(rename = "model", default)]
    pub models: Vec<CatalogModel>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CatalogModel {
    /// Stable kebab-case id, e.g. `qwen3-0.6b-q4`.
    pub id: String,
    pub name: String,
    pub family: String,
    /// Total parameters in billions.
    pub params_b: f64,
    /// Quantisation, e.g. `Q4_K_M`.
    pub quant: String,
    /// SPDX id or the model's own license name; shown before downloading.
    pub license: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// HTTPS download URL, pinned to a commit.
    pub url: String,
    pub size_bytes: u64,
    /// Lowercase hex SHA-256 of the file.
    pub sha256: String,
    pub shape: ModelShape,
    /// The same model in the Ollama library, if there is one (e.g. `qwen3:0.6b`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ollama: Option<String>,
}

impl CatalogModel {
    /// File name to store the model under (last URL segment, restricted to safe characters).
    pub fn file_name(&self) -> String {
        let last = self.url.rsplit('/').next().unwrap_or_default();
        let safe: String = last
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
            .collect();
        if safe.ends_with(".gguf") && !safe.starts_with('.') {
            safe
        } else {
            format!("{}.gguf", self.id)
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum CatalogError {
    #[error("could not parse catalog: {0}")]
    Parse(String),
    #[error("catalog model `{id}`: {message}")]
    Invalid { id: String, message: String },
}

impl Catalog {
    /// The catalog compiled into this build.
    pub fn builtin() -> Self {
        Self::from_toml(BUILTIN).expect("built-in catalog is valid (checked by tests)")
    }

    pub fn from_toml(text: &str) -> Result<Self, CatalogError> {
        let catalog: Catalog =
            toml::from_str(text).map_err(|e| CatalogError::Parse(e.to_string()))?;
        catalog.validate()?;
        Ok(catalog)
    }

    pub fn get(&self, id: &str) -> Option<&CatalogModel> {
        self.models.iter().find(|m| m.id == id)
    }

    fn validate(&self) -> Result<(), CatalogError> {
        if self.schema != infermark_sdk::SCHEMA_VERSION {
            return Err(CatalogError::Parse("unsupported schema version".into()));
        }
        let mut seen = std::collections::HashSet::new();
        for m in &self.models {
            let bad = |message: &str| {
                Err(CatalogError::Invalid {
                    id: m.id.clone(),
                    message: message.into(),
                })
            };
            if !is_model_id(&m.id) {
                return bad("ids use lowercase letters, digits, `-` and `.`");
            }
            if !seen.insert(m.id.as_str()) {
                return bad("duplicate id");
            }
            if !m.url.starts_with("https://") {
                return bad("url must use https");
            }
            if m.sha256.len() != 64
                || !m
                    .sha256
                    .chars()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
            {
                return bad("sha256 must be 64 lowercase hex characters");
            }
            if m.size_bytes == 0 || m.params_b <= 0.0 {
                return bad("size_bytes and params_b must be positive");
            }
            if m.shape.layers == 0 || m.shape.kv_heads == 0 || m.shape.head_dim == 0 {
                return bad("shape values must be positive");
            }
        }
        Ok(())
    }
}

/// Like pack and module ids, plus `.` for version numbers (`llama-3.2-1b-q4`).
pub fn is_model_id(id: &str) -> bool {
    let edge_ok = |c: Option<char>| c.is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    edge_ok(id.chars().next())
        && edge_ok(id.chars().last())
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_catalog_is_valid() {
        let c = Catalog::builtin();
        assert!(c.models.len() >= 5);
        for m in &c.models {
            assert!(m.file_name().ends_with(".gguf"), "{}", m.id);
            assert!(
                m.url.contains("/resolve/"),
                "{} is not a direct download",
                m.id
            );
        }
    }

    #[test]
    fn model_ids() {
        assert!(is_model_id("llama-3.2-1b-q4"));
        for bad in ["", "-x", "x.", "Qwen", "a/b", "a b"] {
            assert!(!is_model_id(bad), "{bad:?}");
        }
    }

    #[test]
    fn file_names_cannot_escape_the_models_folder() {
        let mut m = Catalog::builtin().models[0].clone();
        m.url = "https://example.com/x/..%2F..%2Fevil.gguf".into();
        assert!(!m.file_name().contains('/'));
        m.url = "https://example.com/x/..".into();
        assert_eq!(m.file_name(), format!("{}.gguf", m.id));
    }

    #[test]
    fn rejects_bad_entries() {
        let good = toml::to_string(&Catalog::builtin()).unwrap();
        let first_sha = &Catalog::builtin().models[0].sha256;
        let bad_sha = good.replacen(first_sha.as_str(), "abc", 1);
        assert!(matches!(
            Catalog::from_toml(&bad_sha),
            Err(CatalogError::Invalid { .. })
        ));
        let http = good.replacen("https://", "http://", 1);
        assert!(matches!(
            Catalog::from_toml(&http),
            Err(CatalogError::Invalid { .. })
        ));
    }
}
