//! Built-in benchmark modules.

mod inference_perf;
mod memory_bandwidth;
mod system_probe;

pub use inference_perf::InferencePerf;
pub use memory_bandwidth::MemoryBandwidth;
pub use system_probe::SystemProbe;

use crate::Registry;

pub fn register_builtin(registry: &mut Registry) {
    registry
        .register_module(SystemProbe)
        .register_module(MemoryBandwidth)
        .register_module(InferencePerf);
}
