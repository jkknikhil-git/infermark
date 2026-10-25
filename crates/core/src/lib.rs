//! InferMark core: wires modules together and runs them.
//!
//! ```text
//!   Profile ──┐
//!             ├─► planner::plan ──► Vec<PlanEntry> ──► runner::run ──► RunResult
//!   Hardware ─┤                      (skips + reasons)    (events)
//!   Registry ─┘
//! ```

pub mod model_list;
pub mod modules;
pub mod packs;
pub mod paths;
pub mod planner;
pub mod privacy;
pub mod profiles;
pub mod registry;
pub mod runner;
pub mod schema;
pub mod scorers;
pub mod settings;

pub use planner::plan;
pub use registry::Registry;
pub use runner::{run, RunEvent};

// Re-exported so the CLI and app don't need to depend on them directly.
pub use infermark_engine as engine;
pub use infermark_models as models;

/// Version of this InferMark build, recorded in every result.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
