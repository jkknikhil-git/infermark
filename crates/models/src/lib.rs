//! Everything about models before they are run:
//!
//! * [`catalog`]: the curated list of open-weight GGUF models (`catalog/models.toml`)
//! * [`gguf`]: reads a GGUF header to learn a model's shape (layers, heads, context)
//! * [`gate`]: decides whether a model fits this device and how it will run
//! * [`download`]: HTTPS-only, host-allow-listed, resumable downloads with SHA-256 checks
//! * [`store`]: the local folder that holds downloaded models

pub mod catalog;
pub mod download;
pub mod gate;
pub mod gguf;
pub mod store;

pub use catalog::{Catalog, CatalogModel};
pub use gate::{assess, ModelShape, Tier, Verdict};
pub use store::ModelStore;
