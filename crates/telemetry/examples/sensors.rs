//! Show which sensors this machine offers and a few live readings.
//!
//! ```text
//! cargo run -p infermark-telemetry --example sensors
//! ```

use std::time::Duration;

use infermark_telemetry::{default_providers, Sample, Sampler};

fn main() {
    let sampler = Sampler::start(
        default_providers(),
        Duration::from_secs(1),
        100,
        Some(Box::new(|s: &Sample| {
            let line: Vec<String> = s
                .readings
                .iter()
                .map(|r| match r.value {
                    Some(v) => format!("{}={v:.1}{}", r.key(), r.kind.unit()),
                    None => format!("{}=n/a", r.key()),
                })
                .collect();
            println!("t={:>4.1}s  {}", s.t, line.join("  "));
        })),
    );
    std::thread::sleep(Duration::from_secs(3));
    let rec = sampler.stop();
    println!("\navailable:");
    for c in &rec.inventory.available {
        println!("  {:<22} {}", c.key(), c.label);
    }
    println!("not available on this device:");
    for c in &rec.inventory.missing {
        println!("  {:<22} {}", c.key(), c.label);
    }
}
