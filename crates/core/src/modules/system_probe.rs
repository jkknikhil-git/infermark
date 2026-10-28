use infermark_sdk::{
    BenchModule, CpuCluster, Direction, Metric, ModuleCategory, ModuleDescriptor, ModuleError,
    ModuleOutput, RunContext,
};

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

/// Records the hardware headline numbers and warns about conditions that skew results.
pub struct SystemProbe;

impl BenchModule for SystemProbe {
    fn descriptor(&self) -> ModuleDescriptor {
        ModuleDescriptor {
            id: "system-probe".into(),
            name: "System check".into(),
            description: "Detects your hardware and checks for anything that could skew results."
                .into(),
            category: ModuleCategory::System,
            requirements: vec![],
            estimated_seconds: 1,
        }
    }

    fn run(&self, ctx: &RunContext<'_>) -> Result<ModuleOutput, ModuleError> {
        let hw = ctx.hardware;
        let mut out = ModuleOutput::default();

        out.metrics.push(Metric::new(
            "logical_cores",
            "CPU threads",
            hw.cpu.logical_cores as f64,
            "",
            Direction::Neutral,
        ));
        if let Some(cores) = hw.cpu.physical_cores {
            out.metrics.push(Metric::new(
                "physical_cores",
                "CPU cores",
                cores as f64,
                "",
                Direction::Neutral,
            ));
        }
        out.metrics.push(Metric::new(
            "ram_total_gb",
            "Memory",
            round1(hw.total_ram_gb()),
            "GB",
            Direction::Neutral,
        ));
        out.metrics.push(Metric::new(
            "ram_available_gb",
            "Free memory at start",
            round1(hw.memory.available_bytes as f64 / GIB),
            "GB",
            Direction::Neutral,
        ));
        out.metrics.push(Metric::new(
            "gpu_count",
            "GPUs",
            hw.gpus.len() as f64,
            "",
            Direction::Neutral,
        ));
        let vram = hw.max_dedicated_vram_bytes() as f64 / GIB;
        if vram > 0.0 {
            out.metrics.push(Metric::new(
                "vram_gb",
                "Largest GPU memory",
                round1(vram),
                "GB",
                Direction::Neutral,
            ));
        }
        out.metrics.push(Metric::new(
            "npu_count",
            "NPUs",
            hw.npus.len() as f64,
            "",
            Direction::Neutral,
        ));

        // Memory, power and temperature warnings are the run's pre-run checks
        // (`crate::preflight`), saved in the result; this records the core layout.
        if let Some(fast) = hw.cpu.fast_cores() {
            out.metrics.push(Metric::new(
                "fast_cores",
                "Fast cores",
                fast as f64,
                "",
                Direction::Neutral,
            ));
        }
        if !hw.cpu.clusters.is_empty() {
            out.notes
                .push(format!("Core layout: {}.", layout(&hw.cpu.clusters)));
        }
        if hw.gpus.is_empty() {
            out.notes
                .push("No GPU detected: models will run on the CPU.".into());
        }

        ctx.report(1.0, "Hardware checked");
        Ok(out)
    }
}

/// "1 × 3.3 GHz + 3 × 2.8 GHz + 4 × 1.8 GHz", or "4 cores, 8 threads" for one cluster.
fn layout(clusters: &[CpuCluster]) -> String {
    if let [c] = clusters {
        return if c.logical_cores > c.physical_cores {
            format!("{} cores, {} threads", c.physical_cores, c.logical_cores)
        } else {
            format!("{} cores", c.physical_cores)
        };
    }
    clusters
        .iter()
        .map(|c| match c.max_mhz {
            Some(mhz) => format!("{} × {:.1} GHz", c.physical_cores, mhz as f64 / 1000.0),
            None => format!("{} cores", c.physical_cores),
        })
        .collect::<Vec<_>>()
        .join(" + ")
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(physical: u32, logical: u32, mhz: Option<u32>) -> CpuCluster {
        CpuCluster {
            physical_cores: physical,
            logical_cores: logical,
            max_mhz: mhz,
        }
    }

    #[test]
    fn layouts_read_naturally() {
        assert_eq!(layout(&[c(4, 8, None)]), "4 cores, 8 threads");
        assert_eq!(layout(&[c(8, 8, Some(2400))]), "8 cores");
        assert_eq!(
            layout(&[
                c(1, 1, Some(3300)),
                c(3, 3, Some(2800)),
                c(4, 4, Some(1800))
            ]),
            "1 × 3.3 GHz + 3 × 2.8 GHz + 4 × 1.8 GHz"
        );
        assert_eq!(
            layout(&[c(6, 12, None), c(8, 8, None)]),
            "6 cores + 8 cores"
        );
    }
}
