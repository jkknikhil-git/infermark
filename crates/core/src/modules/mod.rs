//! Built-in benchmark modules.

mod memory_bandwidth;
mod system_probe;

pub use memory_bandwidth::MemoryBandwidth;
pub use system_probe::SystemProbe;

use crate::Registry;

pub fn register_builtin(registry: &mut Registry) {
    registry
        .register_module(SystemProbe)
        .register_module(MemoryBandwidth);
}
