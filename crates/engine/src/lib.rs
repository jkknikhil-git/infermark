//! Inference backends.
//!
//! | Backend | How it runs models |
//! | --- | --- |
//! | [`LlamaServer`] (`llama-cpp`) | Official llama.cpp `llama-server` build, installed by InferMark (pinned + hash-checked) or found on the PATH |
//! | [`Ollama`] (`ollama`) | Models already installed in a local Ollama |

pub mod engines;
pub mod llama_server;
pub mod ollama;
mod stream;

use std::path::PathBuf;

use infermark_sdk::Backend;
use serde::Serialize;

pub use engines::{Accel, EngineBuild, EngineCatalog, EngineStore};
pub use llama_server::LlamaServer;
pub use ollama::{Ollama, OllamaModel};

/// Where to find the engines.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Explicit `llama-server` path from the user's settings; wins over everything else.
    pub llama_server: Option<PathBuf>,
    /// Folder holding builds installed by InferMark.
    pub engines_dir: PathBuf,
    pub ollama_url: String,
}

impl EngineConfig {
    /// The `llama-server` to use: the configured one, else the best installed build, else one on
    /// the PATH.
    pub fn llama_server(&self) -> Option<PathBuf> {
        if let Some(p) = self.llama_server.as_ref().filter(|p| p.is_file()) {
            return Some(p.clone());
        }
        let catalog = EngineCatalog::builtin();
        EngineStore::new(&self.engines_dir)
            .installed(&catalog)
            .into_iter()
            .map(|(_, p)| p)
            .next()
            .or_else(engines::server_on_path)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BackendKind {
    LlamaCpp,
    Ollama,
}

impl BackendKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "llama-cpp" | "llama.cpp" | "llamacpp" => Some(Self::LlamaCpp),
            "ollama" => Some(Self::Ollama),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::LlamaCpp => "llama.cpp",
            Self::Ollama => "Ollama",
        }
    }
}

pub fn open_backend(kind: BackendKind, config: &EngineConfig) -> Result<Box<dyn Backend>, String> {
    match kind {
        BackendKind::LlamaCpp => {
            let exe = config.llama_server().ok_or(
                "llama.cpp is not installed. Install it on the Models page or with `infermark engine install`.",
            )?;
            Ok(Box::new(LlamaServer::new(exe)))
        }
        BackendKind::Ollama => {
            if ollama::version(&config.ollama_url).is_none() {
                return Err(format!("Ollama is not running at {}", config.ollama_url));
            }
            Ok(Box::new(Ollama::new(&config.ollama_url)))
        }
    }
}

/// What is available right now, for the UI and `infermark engine`.
#[derive(Debug, Clone, Serialize)]
pub struct EngineStatus {
    pub llama_cpp: Option<LlamaCppStatus>,
    pub ollama: Option<OllamaStatus>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LlamaCppStatus {
    pub path: PathBuf,
    pub version: String,
    /// `None` for a build InferMark didn't install (custom path or PATH).
    pub accel: Option<Accel>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OllamaStatus {
    pub url: String,
    pub version: String,
    pub models: Vec<OllamaModel>,
}

pub fn status(config: &EngineConfig) -> EngineStatus {
    let catalog = EngineCatalog::builtin();
    let installed = EngineStore::new(&config.engines_dir).installed(&catalog);
    let llama_cpp = config.llama_server().map(|path| LlamaCppStatus {
        version: llama_server::server_version(&path).unwrap_or_else(|| "unknown".into()),
        accel: installed
            .iter()
            .find(|(_, p)| *p == path)
            .map(|(b, _)| b.accel),
        path,
    });
    let ollama = ollama::version(&config.ollama_url).map(|version| OllamaStatus {
        url: config.ollama_url.clone(),
        version,
        models: ollama::list_models(&config.ollama_url).unwrap_or_default(),
    });
    EngineStatus { llama_cpp, ollama }
}

/// Turn a ureq error into a readable message, including the server's error body.
pub(crate) fn http_error(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, r) => {
            let body = r.into_string().unwrap_or_default();
            let msg = serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|v| {
                    v["error"]
                        .as_str()
                        .map(String::from)
                        .or_else(|| v["error"]["message"].as_str().map(String::from))
                })
                .unwrap_or(body);
            format!("HTTP {code}: {}", msg.trim())
        }
        other => other.to_string(),
    }
}
