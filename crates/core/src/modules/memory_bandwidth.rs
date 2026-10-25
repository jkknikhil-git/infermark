//! STREAM-style memory bandwidth test.
//!
//! Token generation in LLMs is mostly limited by how fast weights can be read from memory, so
//! this number is the best single predictor of tokens/sec (and feeds the hardware gate).

use std::time::Instant;

use infermark_sdk::{
    BenchModule, Direction, Metric, ModuleCategory, ModuleDescriptor, ModuleError, ModuleOutput,
    RunContext, Series,
};

const MIB: u64 = 1024 * 1024;

pub struct MemoryBandwidth;

impl BenchModule for MemoryBandwidth {
    fn descriptor(&self) -> ModuleDescriptor {
        ModuleDescriptor {
            id: "memory-bandwidth".into(),
            name: "Memory speed".into(),
            description:
                "Measures how fast data moves between memory and the CPU (copy and triad).".into(),
            category: ModuleCategory::Microbenchmark,
            requirements: vec![],
            estimated_seconds: 10,
        }
    }

    /// Settings: `size_mb` (per array, default 256), `iterations` (default 5),
    /// `threads` (default: all logical cores).
    fn run(&self, ctx: &RunContext<'_>) -> Result<ModuleOutput, ModuleError> {
        let requested_mb = ctx.setting_u64("size_mb", 256);
        let iterations = ctx.setting_u64("iterations", 5).max(1);
        let threads = ctx
            .setting_u64("threads", ctx.hardware.cpu.logical_cores as u64)
            .max(1) as usize;
        if requested_mb == 0 {
            return Err(ModuleError::InvalidSetting {
                key: "size_mb".into(),
                message: "must be at least 1".into(),
            });
        }

        // Three arrays; never use more than a quarter of the memory that is free right now.
        let cap_mb = (ctx.hardware.memory.available_bytes / 4 / 3 / MIB).max(16);
        let size_mb = requested_mb.min(cap_mb);
        let n = (size_mb * MIB / 8) as usize;

        let mut out = ModuleOutput::default();
        if size_mb < requested_mb {
            out.notes.push(format!(
                "Array size reduced to {size_mb} MB to fit in free memory."
            ));
        }

        let mut a = vec![0.0f64; n];
        let b = vec![1.0f64; n];
        let c = vec![2.0f64; n];
        // Touch `a` once so page faults aren't timed.
        parallel(&mut a, &b, &c, threads, |a, b, _| a.copy_from_slice(b));

        let mut best_copy = f64::MAX;
        let mut best_triad = f64::MAX;
        let mut triad_series = Vec::new();
        let scalar = 3.0;

        for i in 0..iterations {
            if ctx.is_cancelled() {
                return Err(ModuleError::Cancelled);
            }
            let t = Instant::now();
            parallel(&mut a, &b, &c, threads, |a, b, _| a.copy_from_slice(b));
            best_copy = best_copy.min(t.elapsed().as_secs_f64());

            let t = Instant::now();
            parallel(&mut a, &b, &c, threads, |a, b, c| {
                for ((x, y), z) in a.iter_mut().zip(b).zip(c) {
                    *x = y + scalar * z;
                }
            });
            let secs = t.elapsed().as_secs_f64();
            best_triad = best_triad.min(secs);
            triad_series.push(((i + 1) as f64, gbps(3 * n, secs)));

            ctx.report(
                (i + 1) as f64 / iterations as f64,
                format!("Pass {} of {iterations}", i + 1),
            );
        }
        std::hint::black_box(&a);

        out.metrics.push(Metric::new(
            "copy_gbps",
            "Copy bandwidth",
            gbps(2 * n, best_copy),
            "GB/s",
            Direction::HigherIsBetter,
        ));
        out.metrics.push(Metric::new(
            "triad_gbps",
            "Triad bandwidth",
            gbps(3 * n, best_triad),
            "GB/s",
            Direction::HigherIsBetter,
        ));
        out.metrics.push(Metric::new(
            "array_mb",
            "Array size",
            size_mb as f64,
            "MB",
            Direction::Neutral,
        ));
        out.metrics.push(Metric::new(
            "threads",
            "Threads",
            threads as f64,
            "",
            Direction::Neutral,
        ));
        out.series.push(Series {
            key: "triad_by_pass".into(),
            label: "Triad bandwidth per pass".into(),
            x_unit: "pass".into(),
            y_unit: "GB/s".into(),
            points: triad_series,
        });
        Ok(out)
    }
}

/// Split the arrays into one chunk per thread and run `kernel` on each chunk.
fn parallel(
    a: &mut [f64],
    b: &[f64],
    c: &[f64],
    threads: usize,
    kernel: impl Fn(&mut [f64], &[f64], &[f64]) + Sync,
) {
    let chunk = a.len().div_ceil(threads).max(1);
    std::thread::scope(|s| {
        for ((a, b), c) in a
            .chunks_mut(chunk)
            .zip(b.chunks(chunk))
            .zip(c.chunks(chunk))
        {
            let kernel = &kernel;
            s.spawn(move || kernel(a, b, c));
        }
    });
}

/// GB/s (10^9 bytes, as STREAM reports) for `elements` f64 values moved in `secs`.
fn gbps(elements: usize, secs: f64) -> f64 {
    let gb = (elements * 8) as f64 / 1e9;
    ((gb / secs.max(1e-9)) * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parallel_triad_matches_serial() {
        let n = 10_001;
        let mut a = vec![0.0; n];
        let b: Vec<f64> = (0..n).map(|i| i as f64).collect();
        let c = vec![2.0; n];
        parallel(&mut a, &b, &c, 7, |a, b, c| {
            for ((x, y), z) in a.iter_mut().zip(b).zip(c) {
                *x = y + 3.0 * z;
            }
        });
        assert!(a.iter().enumerate().all(|(i, v)| *v == i as f64 + 6.0));
    }

    #[test]
    fn gbps_math() {
        // 125M f64 = 1 GB in 0.5 s = 2 GB/s
        assert_eq!(gbps(125_000_000, 0.5), 2.0);
    }
}
