//! The modularity contract: profiles choose modules, requirements gate them, packs plug in,
//! and nothing a module does can take the whole run down.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

use infermark_core::{packs, plan, run, schema, Registry, RunEvent};
use infermark_sdk::{
    BenchModule, CpuInfo, GpuInfo, GpuVendor, HardwareInfo, MemoryInfo, ModuleCategory,
    ModuleDescriptor, ModuleError, ModuleOutput, ModuleStatus, OsFamily, OsInfo, PlanStatus,
    PowerSource, Profile, Requirement, RunContext,
};

const GIB: u64 = 1024 * 1024 * 1024;

/// An Iris Xe laptop like the maintainer's: 16 GB, Intel integrated GPU, no NVIDIA.
fn intel_laptop() -> HardwareInfo {
    HardwareInfo {
        fingerprint: "test".into(),
        os: OsInfo {
            family: OsFamily::Windows,
            name: "Windows".into(),
            version: "11".into(),
            kernel: "26200".into(),
            arch: "x86_64".into(),
        },
        cpu: CpuInfo {
            brand: "11th Gen Intel(R) Core(TM) i5-11320H".into(),
            vendor: "GenuineIntel".into(),
            physical_cores: Some(4),
            logical_cores: 8,
            frequency_mhz: Some(3200),
        },
        memory: MemoryInfo {
            total_bytes: 16 * GIB,
            available_bytes: 8 * GIB,
        },
        gpus: vec![GpuInfo {
            name: "Intel(R) Iris(R) Xe Graphics".into(),
            vendor: GpuVendor::Intel,
            dedicated_vram_bytes: Some(128 * 1024 * 1024),
            shared_memory_bytes: Some(8 * GIB),
            driver_version: None,
            integrated: true,
        }],
        npus: vec![],
        battery: None,
        power_source: PowerSource::Ac,
    }
}

struct Mock {
    id: &'static str,
    requirements: Vec<Requirement>,
    behaviour: fn(&RunContext<'_>) -> Result<ModuleOutput, ModuleError>,
}

impl BenchModule for Mock {
    fn descriptor(&self) -> ModuleDescriptor {
        ModuleDescriptor {
            id: self.id.into(),
            name: self.id.into(),
            description: String::new(),
            category: ModuleCategory::Microbenchmark,
            requirements: self.requirements.clone(),
            estimated_seconds: 0,
        }
    }

    fn run(&self, ctx: &RunContext<'_>) -> Result<ModuleOutput, ModuleError> {
        (self.behaviour)(ctx)
    }
}

fn ok(_: &RunContext<'_>) -> Result<ModuleOutput, ModuleError> {
    Ok(ModuleOutput::default())
}

fn echo_setting(ctx: &RunContext<'_>) -> Result<ModuleOutput, ModuleError> {
    Ok(ModuleOutput {
        notes: vec![format!("size={}", ctx.setting_u64("size", 0))],
        ..Default::default()
    })
}

fn boom(_: &RunContext<'_>) -> Result<ModuleOutput, ModuleError> {
    Err(ModuleError::Failed("boom".into()))
}

fn registry() -> Registry {
    let mut r = Registry::builtin();
    r.register_module(Mock {
        id: "always",
        requirements: vec![],
        behaviour: echo_setting,
    })
    .register_module(Mock {
        id: "nvidia-only",
        requirements: vec![Requirement::GpuVendor {
            vendor: GpuVendor::Nvidia,
        }],
        behaviour: ok,
    })
    .register_module(Mock {
        id: "big-ram",
        requirements: vec![Requirement::MinRamGb { gb: 64.0 }],
        behaviour: ok,
    })
    .register_module(Mock {
        id: "fails",
        requirements: vec![],
        behaviour: boom,
    });
    r
}

fn profile(body: &str) -> Profile {
    Profile::from_toml(&format!("schema = 1\nname = \"Test\"\n{body}")).unwrap()
}

#[test]
fn disabling_a_module_removes_it_from_the_plan() {
    let p = profile("[modules.always]\n[modules.fails]\nenabled = false\n");
    let entries = plan(&p, &registry(), &intel_laptop());
    let ids: Vec<_> = entries.iter().map(|e| e.module_id.as_str()).collect();
    assert_eq!(ids, ["always"]);
}

#[test]
fn modules_needing_missing_hardware_are_skipped_with_a_reason() {
    let p = profile("[modules.nvidia-only]\n[modules.big-ram]\n[modules.always]\n");
    let entries = plan(&p, &registry(), &intel_laptop());
    let reason = |i: usize| match &entries[i].status {
        PlanStatus::Skipped { reason } => reason.clone(),
        PlanStatus::Planned => panic!("{} should be skipped", entries[i].module_id),
    };
    assert_eq!(reason(0), "needs a NVIDIA GPU");
    assert!(reason(1).contains("needs 64 GB RAM"), "{}", reason(1));
    assert_eq!(entries[2].status, PlanStatus::Planned);
}

#[test]
fn unknown_modules_are_skipped_not_fatal() {
    let p = profile("[modules.from-the-future]\n");
    let entries = plan(&p, &registry(), &intel_laptop());
    assert!(entries[0].descriptor.is_none());
    assert!(
        matches!(&entries[0].status, PlanStatus::Skipped { reason } if reason.contains("not available"))
    );
}

#[test]
fn run_passes_settings_and_survives_failures() {
    let p = profile("[modules.fails]\n[modules.always]\nsize = 64\n[modules.nvidia-only]\n");
    let events = Mutex::new(Vec::new());
    let on_event = |e: RunEvent| events.lock().unwrap().push(e);
    let result = run(
        &p,
        &registry(),
        &intel_laptop(),
        &on_event,
        &AtomicBool::new(false),
    );

    let statuses: Vec<_> = result.modules.iter().map(|m| &m.status).collect();
    assert!(matches!(statuses[0], ModuleStatus::Failed { error } if error == "boom"));
    assert_eq!(statuses[1], &ModuleStatus::Completed);
    assert!(matches!(statuses[2], ModuleStatus::Skipped { .. }));
    assert_eq!(result.modules[1].output.notes, ["size=64"]);
    // started + finished for each module
    assert_eq!(events.lock().unwrap().len(), 6);
}

#[test]
fn cancel_stops_remaining_modules() {
    let p = profile("[modules.always]\n[modules.fails]\n");
    let result = run(
        &p,
        &registry(),
        &intel_laptop(),
        &|_| {},
        &AtomicBool::new(true),
    );
    assert!(result
        .modules
        .iter()
        .all(|m| m.status == ModuleStatus::Cancelled));
}

#[test]
fn builtin_modules_run_on_this_machine() {
    let p = profile(
        "[modules.system-probe]\n[modules.memory-bandwidth]\nsize_mb = 8\niterations = 1\n",
    );
    let hw = infermark_hwprobe::probe();
    let result = run(
        &p,
        &Registry::builtin(),
        &hw,
        &|_| {},
        &AtomicBool::new(false),
    );
    for m in &result.modules {
        assert_eq!(m.status, ModuleStatus::Completed, "{}", m.module_id);
    }
    let triad = result.modules[1]
        .output
        .metrics
        .iter()
        .find(|m| m.key == "triad_gbps")
        .unwrap();
    assert!(triad.value > 0.0);
    // Results round-trip through JSON.
    let json = serde_json::to_string(&result).unwrap();
    let back: infermark_sdk::RunResult = serde_json::from_str(&json).unwrap();
    assert_eq!(back.run_id, result.run_id);
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/packs")
}

#[test]
fn sample_pack_loads_and_scores() {
    let r = Registry::builtin();
    let pack = packs::load(&fixtures().join("sample-trivia"), &r).unwrap();
    assert_eq!(pack.items.len(), 3);
    let scorer = r.scorer(&pack.manifest.scorer).unwrap();
    let item = &pack.items[0];
    assert!(scorer.score(&pack.manifest, item, "B").passed);
    let prompt = item.render(&pack.manifest.prompt);
    assert!(prompt.contains("B) Mercury"), "{prompt}");
}

#[test]
fn broken_pack_reports_every_problem() {
    let err = packs::load(&fixtures().join("broken"), &Registry::builtin()).unwrap_err();
    let all = err.issues.join("\n");
    for needle in [
        "kebab-case",
        "`license` must not be empty",
        "unknown scorer",
        "duplicate id",
        "line 3",
    ] {
        assert!(all.contains(needle), "missing {needle:?} in:\n{all}");
    }
}

#[test]
fn discover_skips_broken_and_selects_by_profile() {
    let (found, errors) = packs::discover(&[fixtures()], &Registry::builtin());
    assert_eq!(found.len(), 1);
    assert_eq!(errors.len(), 1);
    let p = profile("[packs]\ninclude = [\"*\"]\nexclude = [\"sample-trivia\"]\n");
    assert!(packs::select(&found, &p.packs).is_empty());
    let p = profile("[packs]\ninclude = [\"sample-trivia\"]\n");
    assert_eq!(packs::select(&found, &p.packs).len(), 1);
}

#[test]
fn committed_schemas_are_up_to_date() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schema");
    for (name, generated) in schema::all() {
        let committed = std::fs::read_to_string(dir.join(name)).unwrap_or_default();
        assert!(
            committed.replace("\r\n", "\n") == generated,
            "schema/{name} is out of date; run `cargo run -p infermark-cli -- schema export`"
        );
    }
}
