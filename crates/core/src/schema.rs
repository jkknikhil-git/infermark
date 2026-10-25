//! JSON Schemas for the files InferMark reads and writes. Generated from the Rust types so they
//! can never drift; `infermark schema export` writes them to `schema/`.

use infermark_sdk::{PackItem, PackManifest, Profile, RunResult};
use schemars::schema_for;

/// `(file name, pretty JSON)` for every published schema.
pub fn all() -> Vec<(&'static str, String)> {
    let pretty = |s| serde_json::to_string_pretty(&s).expect("schema serializes") + "\n";
    vec![
        ("result.v1.json", pretty(schema_for!(RunResult))),
        ("profile.v1.json", pretty(schema_for!(Profile))),
        ("pack.v1.json", pretty(schema_for!(PackManifest))),
        ("pack-item.v1.json", pretty(schema_for!(PackItem))),
    ]
}
