//! Ollama backend: benchmarks models the user already has in Ollama, over its local HTTP API.

use std::io::{BufRead, BufReader};
use std::time::{Duration, Instant};

use infermark_sdk::{
    Backend, GenerateRequest, GenerationStats, LoadOptions, ModelSpec, TokenEvent,
};
use serde::Serialize;
use serde_json::{json, Value};

use crate::http_error;
use crate::stream::StreamTracker;

pub const DEFAULT_URL: &str = "http://127.0.0.1:11434";

pub struct Ollama {
    base: String,
    agent: ureq::Agent,
    version: String,
    loaded: Option<Loaded>,
}

struct Loaded {
    model: String,
    options: LoadOptions,
    load_ms: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct OllamaModel {
    /// Tag, e.g. `llama3.2:3b`.
    pub name: String,
    pub size_bytes: u64,
    pub parameter_size: String,
    pub quantization: String,
}

fn agent(read_timeout: Duration) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(3))
        .timeout_read(read_timeout)
        .build()
}

/// Ollama's version if it is running at `base`.
pub fn version(base: &str) -> Option<String> {
    let r = agent(Duration::from_secs(3))
        .get(&format!("{}/api/version", base.trim_end_matches('/')))
        .call()
        .ok()?;
    let v: Value = serde_json::from_reader(r.into_reader()).ok()?;
    v["version"].as_str().map(String::from)
}

/// Models installed in Ollama.
pub fn list_models(base: &str) -> Result<Vec<OllamaModel>, String> {
    let r = agent(Duration::from_secs(10))
        .get(&format!("{}/api/tags", base.trim_end_matches('/')))
        .call()
        .map_err(http_error)?;
    let v: Value = serde_json::from_reader(r.into_reader()).map_err(|e| e.to_string())?;
    Ok(v["models"]
        .as_array()
        .map(|models| {
            models
                .iter()
                .filter_map(|m| {
                    Some(OllamaModel {
                        name: m["name"].as_str()?.to_string(),
                        size_bytes: m["size"].as_u64().unwrap_or(0),
                        parameter_size: m["details"]["parameter_size"]
                            .as_str()
                            .unwrap_or("")
                            .into(),
                        quantization: m["details"]["quantization_level"]
                            .as_str()
                            .unwrap_or("")
                            .into(),
                    })
                })
                .collect()
        })
        .unwrap_or_default())
}

impl Ollama {
    pub fn new(base: &str) -> Self {
        let base = base.trim_end_matches('/').to_string();
        Self {
            version: version(&base).unwrap_or_else(|| "unknown".into()),
            agent: agent(Duration::from_secs(600)),
            base,
            loaded: None,
        }
    }

    fn options(&self, options: &LoadOptions, extra: Value) -> Value {
        let mut o = json!({ "num_ctx": options.context_size.max(256), "seed": options.seed });
        if let Some(t) = options.threads {
            o["num_thread"] = json!(t);
        }
        if let Some(n) = options.gpu_layers {
            o["num_gpu"] = json!(n);
        }
        if let (Some(o), Some(extra)) = (o.as_object_mut(), extra.as_object()) {
            o.extend(extra.clone());
        }
        o
    }
}

impl Backend for Ollama {
    fn id(&self) -> &'static str {
        "ollama"
    }

    fn version(&self) -> String {
        self.version.clone()
    }

    fn load(&mut self, model: &ModelSpec, options: &LoadOptions) -> Result<(), String> {
        self.unload();
        let installed = list_models(&self.base)
            .map_err(|e| format!("Ollama is not reachable at {}: {e}", self.base))?;
        if !installed.iter().any(|m| m.name == model.location) {
            return Err(format!(
                "`{}` is not installed in Ollama (run `ollama pull {}`)",
                model.location, model.location
            ));
        }
        // An empty prompt loads the model without generating anything.
        let start = Instant::now();
        self.agent
            .post(&format!("{}/api/generate", self.base))
            .send_json(json!({
                "model": model.location,
                "prompt": "",
                "stream": false,
                "keep_alive": "10m",
                "options": self.options(options, json!({})),
            }))
            .map_err(http_error)?;
        self.loaded = Some(Loaded {
            model: model.location.clone(),
            options: options.clone(),
            load_ms: start.elapsed().as_secs_f64() * 1000.0,
        });
        Ok(())
    }

    fn generate(
        &mut self,
        request: &GenerateRequest,
        on_token: &mut dyn FnMut(TokenEvent),
    ) -> Result<GenerationStats, String> {
        let loaded = self.loaded.as_ref().ok_or("no model loaded")?;
        let mut body = json!({
            "model": loaded.model,
            "prompt": request.prompt,
            "stream": true,
            "keep_alive": "10m",
            "options": self.options(&loaded.options, json!({
                "num_predict": request.max_tokens,
                "temperature": request.temperature,
                "stop": request.stop,
            })),
        });
        if let Some(system) = &request.system {
            body["system"] = json!(system);
        }
        if request.think {
            body["think"] = json!(true);
        }
        let start = Instant::now();
        let response = self
            .agent
            .post(&format!("{}/api/generate", self.base))
            .send_json(body)
            .map_err(http_error)?;

        let mut tracker = StreamTracker::new(start);
        for line in BufReader::new(response.into_reader()).lines() {
            let line = line.map_err(|e| format!("stream interrupted: {e}"))?;
            if line.trim().is_empty() {
                continue;
            }
            let chunk: Value =
                serde_json::from_str(&line).map_err(|e| format!("bad chunk: {e}"))?;
            if let Some(err) = chunk.get("error") {
                return Err(format!("Ollama: {err}"));
            }
            if let Some(ev) = chunk["thinking"]
                .as_str()
                .and_then(|t| tracker.piece(t, Some(true)))
            {
                on_token(ev);
            }
            // When Ollama separates thinking, `response` is always the answer.
            let separated = request.think.then_some(false);
            if let Some(ev) = chunk["response"]
                .as_str()
                .and_then(|t| tracker.piece(t, separated))
            {
                on_token(ev);
            }
            if chunk["done"].as_bool() == Some(true) {
                let ns = |k: &str| chunk[k].as_f64().unwrap_or(0.0) / 1e6;
                return Ok(GenerationStats {
                    prompt_tokens: chunk["prompt_eval_count"].as_u64().unwrap_or(0) as u32,
                    output_tokens: chunk["eval_count"].as_u64().unwrap_or(0) as u32,
                    thinking_tokens: tracker.thinking_pieces,
                    load_ms: loaded.load_ms,
                    prompt_eval_ms: ns("prompt_eval_duration"),
                    generation_ms: ns("eval_duration"),
                    time_to_first_token_ms: tracker
                        .first_token_ms
                        .unwrap_or_else(|| tracker.elapsed_ms()),
                    time_to_first_visible_token_ms: tracker.first_visible_ms,
                });
            }
        }
        Err("Ollama ended the stream early".into())
    }

    fn unload(&mut self) {
        if let Some(l) = self.loaded.take() {
            let _ = self
                .agent
                .post(&format!("{}/api/generate", self.base))
                .send_json(json!({ "model": l.model, "keep_alive": 0 }));
        }
    }
}

impl Drop for Ollama {
    fn drop(&mut self) {
        self.unload();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unreachable_server_is_reported() {
        // Port 9 (discard) is essentially never an HTTP server.
        assert_eq!(version("http://127.0.0.1:9"), None);
        let mut o = Ollama::new("http://127.0.0.1:9/");
        let err = o
            .load(
                &ModelSpec {
                    id: "x".into(),
                    location: "x:1b".into(),
                    sha256: None,
                },
                &LoadOptions::default(),
            )
            .unwrap_err();
        assert!(err.contains("not reachable"), "{err}");
    }
}
