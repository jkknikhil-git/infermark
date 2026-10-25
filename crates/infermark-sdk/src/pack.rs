//! Test packs: declarative quality tests anyone can write (a `pack.toml` plus an `items.jsonl`).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Contents of `pack.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PackManifest {
    /// File format version; must be 1.
    pub schema: u32,
    /// Stable kebab-case id, unique across installed packs.
    pub id: String,
    pub name: String,
    /// Semantic version of the pack contents (bump when items change).
    pub version: String,
    #[serde(default)]
    pub description: String,
    /// Grouping shown in results, e.g. `coding`, `reasoning`, `summarization`.
    pub category: String,
    /// Scorer id, e.g. `exact_match`, `multiple_choice`, `regex`, `json_schema`, `llm_judge`.
    pub scorer: String,
    /// Extra options passed to the scorer.
    #[serde(default)]
    #[schemars(with = "std::collections::BTreeMap<String, serde_json::Value>")]
    pub scorer_options: toml::Table,
    /// SPDX license of the items (e.g. `Apache-2.0`, `CC-BY-4.0`).
    pub license: String,
    /// True when scores are opinion-based (creative writing); shown with a label in the UI.
    #[serde(default)]
    pub subjective: bool,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub prompt: PromptTemplate,
    #[serde(default)]
    pub generation: GenerationSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PromptTemplate {
    #[serde(default)]
    pub system: Option<String>,
    /// User message template. Placeholders: `{{input}}`, `{{choices}}`.
    #[serde(default = "d_template")]
    pub template: String,
}

impl Default for PromptTemplate {
    fn default() -> Self {
        Self {
            system: None,
            template: d_template(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GenerationSettings {
    #[serde(default = "d_max_tokens")]
    pub max_tokens: u32,
    #[serde(default)]
    pub temperature: f32,
    #[serde(default)]
    pub stop: Vec<String>,
}

impl Default for GenerationSettings {
    fn default() -> Self {
        Self {
            max_tokens: d_max_tokens(),
            temperature: 0.0,
            stop: Vec::new(),
        }
    }
}

/// One line of `items.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PackItem {
    pub id: String,
    pub input: String,
    /// Accepted answer(s). A single string or a list of alternatives.
    #[serde(default)]
    pub expected: Option<Expected>,
    /// Options for multiple-choice items, rendered as `A) …`, `B) …`.
    #[serde(default)]
    pub choices: Option<Vec<String>>,
    /// Grading instructions for judge-based scorers.
    #[serde(default)]
    pub rubric: Option<String>,
    /// Test code for `code_exec` items.
    #[serde(default)]
    pub tests: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(untagged)]
pub enum Expected {
    One(String),
    Many(Vec<String>),
}

impl Expected {
    pub fn answers(&self) -> Vec<&str> {
        match self {
            Self::One(s) => vec![s.as_str()],
            Self::Many(v) => v.iter().map(String::as_str).collect(),
        }
    }
}

impl PackItem {
    /// Fill the pack's template with this item.
    pub fn render(&self, template: &PromptTemplate) -> String {
        let choices = self
            .choices
            .as_ref()
            .map(|c| {
                c.iter()
                    .enumerate()
                    .map(|(i, text)| format!("{}) {text}", char::from(b'A' + i as u8)))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        template
            .template
            .replace("{{input}}", &self.input)
            .replace("{{choices}}", &choices)
    }
}

fn d_template() -> String {
    "{{input}}".into()
}
fn d_max_tokens() -> u32 {
    512
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expected_accepts_string_or_list() {
        let a: PackItem = serde_json::from_str(r#"{"id":"a","input":"q","expected":"x"}"#).unwrap();
        let b: PackItem =
            serde_json::from_str(r#"{"id":"b","input":"q","expected":["x","y"]}"#).unwrap();
        assert_eq!(a.expected.unwrap().answers(), ["x"]);
        assert_eq!(b.expected.unwrap().answers(), ["x", "y"]);
    }

    #[test]
    fn renders_choices() {
        let item: PackItem =
            serde_json::from_str(r#"{"id":"a","input":"Pick","choices":["red","blue"]}"#).unwrap();
        let t = PromptTemplate {
            system: None,
            template: "{{input}}\n{{choices}}".into(),
        };
        assert_eq!(item.render(&t), "Pick\nA) red\nB) blue");
    }
}
