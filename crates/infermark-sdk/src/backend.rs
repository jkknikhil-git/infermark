//! Inference backends (llama.cpp, Ollama, …).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub trait Backend: Send {
    /// Stable id used in profiles (`run.backend = "llama-cpp"`).
    fn id(&self) -> &'static str;

    /// Engine version string recorded in results for reproducibility.
    fn version(&self) -> String;

    fn load(&mut self, model: &ModelSpec, options: &LoadOptions) -> Result<(), String>;

    /// Generate a completion, calling `on_token` for every streamed piece of text.
    fn generate(
        &mut self,
        request: &GenerateRequest,
        on_token: &mut dyn FnMut(TokenEvent),
    ) -> Result<GenerationStats, String>;

    fn unload(&mut self);
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct ModelSpec {
    pub id: String,
    /// Local file path (llama.cpp) or tag (Ollama).
    pub location: String,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct LoadOptions {
    pub context_size: u32,
    pub threads: Option<u32>,
    pub gpu_layers: Option<i32>,
    pub seed: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct GenerateRequest {
    pub system: Option<String>,
    pub prompt: String,
    pub max_tokens: u32,
    pub temperature: f32,
    pub stop: Vec<String>,
    /// Allow reasoning output for models that support it.
    pub think: bool,
    /// Keep generating past the end-of-text token so speed tests always produce
    /// `max_tokens` tokens. Backends that can't do this generate as many as the model wants.
    #[serde(default)]
    pub ignore_eos: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TokenEvent {
    pub text: String,
    /// True for reasoning ("thinking") tokens, which are counted separately from the answer.
    pub thinking: bool,
}

/// Timings for one generation. All durations in milliseconds.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct GenerationStats {
    pub prompt_tokens: u32,
    pub output_tokens: u32,
    /// Subset of `output_tokens` that were reasoning tokens.
    pub thinking_tokens: u32,
    pub load_ms: f64,
    pub prompt_eval_ms: f64,
    pub generation_ms: f64,
    pub time_to_first_token_ms: f64,
    /// First token of the visible answer (after any reasoning).
    pub time_to_first_visible_token_ms: Option<f64>,
}

impl GenerationStats {
    pub fn prompt_tokens_per_sec(&self) -> f64 {
        per_sec(self.prompt_tokens, self.prompt_eval_ms)
    }

    pub fn generation_tokens_per_sec(&self) -> f64 {
        per_sec(self.output_tokens, self.generation_ms)
    }
}

fn per_sec(tokens: u32, ms: f64) -> f64 {
    if ms > 0.0 {
        tokens as f64 / (ms / 1000.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_per_second() {
        let s = GenerationStats {
            prompt_tokens: 512,
            output_tokens: 128,
            prompt_eval_ms: 500.0,
            generation_ms: 4000.0,
            ..Default::default()
        };
        assert_eq!(s.prompt_tokens_per_sec(), 1024.0);
        assert_eq!(s.generation_tokens_per_sec(), 32.0);
        assert_eq!(GenerationStats::default().generation_tokens_per_sec(), 0.0);
    }
}
