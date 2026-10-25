use infermark_sdk::{
    BenchModule, Direction, Metric, ModuleCategory, ModuleDescriptor, ModuleError, ModuleOutput,
    PowerSource, RunContext,
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

        // Pre-flight warnings.
        let free_fraction = hw.memory.available_bytes as f64 / hw.memory.total_bytes.max(1) as f64;
        if free_fraction < 0.3 {
            out.notes.push(format!(
                "Only {:.0}% of memory is free. Close other apps for more accurate results.",
                free_fraction * 100.0
            ));
        }
        if hw.power_source == PowerSource::Battery {
            out.notes
                .push("Running on battery: performance may be limited by power saving.".into());
        }
        if let Some(b) = &hw.battery {
            if b.charge_percent < 30.0 && !b.charging {
                out.notes.push(format!(
                    "Battery is at {:.0}%. Plug in for long runs.",
                    b.charge_percent
                ));
            }
        }
        if hw.gpus.is_empty() {
            out.notes
                .push("No GPU detected: models will run on the CPU.".into());
        }

        ctx.report(1.0, "Hardware checked");
        Ok(out)
    }
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}
