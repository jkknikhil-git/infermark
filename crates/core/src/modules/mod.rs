//! Built-in benchmark modules.

mod battery_drain;
mod inference_perf;
mod memory_bandwidth;
mod model_run;
mod sustained_load;
mod system_probe;
mod thread_sweep;

pub use battery_drain::BatteryDrain;
pub use inference_perf::InferencePerf;
pub use memory_bandwidth::MemoryBandwidth;
pub use sustained_load::SustainedLoad;
pub use system_probe::SystemProbe;
pub use thread_sweep::ThreadSweep;

use crate::Registry;

pub fn register_builtin(registry: &mut Registry) {
    registry
        .register_module(SystemProbe)
        .register_module(MemoryBandwidth)
        .register_module(InferencePerf)
        .register_module(ThreadSweep)
        .register_module(SustainedLoad)
        .register_module(BatteryDrain);
}
