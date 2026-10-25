//! Result exporters (JSON, CSV, HTML, …).

use std::io::Write;

use crate::result::RunResult;

pub trait Exporter: Send + Sync {
    fn id(&self) -> &'static str;

    /// File extension without the dot, e.g. `json`.
    fn extension(&self) -> &'static str;

    fn export(&self, result: &RunResult, out: &mut dyn Write) -> std::io::Result<()>;
}
