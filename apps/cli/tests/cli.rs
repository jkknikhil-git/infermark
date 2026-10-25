//! Every command and flag of the CLI, exercised end to end against an isolated INFERMARK_HOME.

use std::path::Path;
use std::process::{Command, Output};

fn infermark(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_infermark"))
        .args(args)
        .env("INFERMARK_HOME", home)
        .output()
        .expect("binary runs")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).to_string()
}

fn fixtures() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/core/tests/fixtures/packs")
}

#[test]
fn probe_text_and_json() {
    let home = tempfile::tempdir().unwrap();
    let text = infermark(home.path(), &["probe"]);
    assert!(text.status.success());
    assert!(stdout(&text).contains("CPU"));
    let json = infermark(home.path(), &["probe", "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert!(v["cpu"]["logical_cores"].as_u64().unwrap() >= 1);
}

#[test]
fn profiles_include_user_files_and_report_bad_ones() {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join("profiles");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("mine.toml"),
        "schema = 1\nname = \"Mine\"\n[modules.system-probe]\n",
    )
    .unwrap();
    std::fs::write(dir.join("bad.toml"), "schema = 1\n").unwrap();

    let out = infermark(home.path(), &["profiles"]);
    assert!(stdout(&out).contains("Mine"));
    assert!(String::from_utf8_lossy(&out.stderr).contains("bad.toml"));

    let json = infermark(home.path(), &["profiles", "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    let keys: Vec<_> = v
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["key"].as_str().unwrap().to_string())
        .collect();
    assert!(keys.contains(&"quick".to_string()) && keys.contains(&"mine".to_string()));

    assert!(infermark(
        home.path(),
        &[
            "profile",
            "validate",
            dir.join("mine.toml").to_str().unwrap()
        ]
    )
    .status
    .success());
    assert!(!infermark(
        home.path(),
        &[
            "profile",
            "validate",
            dir.join("bad.toml").to_str().unwrap()
        ]
    )
    .status
    .success());
}

#[test]
fn modules_lists_builtins() {
    let home = tempfile::tempdir().unwrap();
    assert!(stdout(&infermark(home.path(), &["modules"])).contains("memory-bandwidth"));
    let v: serde_json::Value =
        serde_json::from_slice(&infermark(home.path(), &["modules", "--json"]).stdout).unwrap();
    assert!(v
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["id"] == "system-probe"));
}

#[test]
fn plan_shows_skips_with_reasons() {
    let home = tempfile::tempdir().unwrap();
    let out = infermark(home.path(), &["plan", "--profile", "quick"]);
    let text = stdout(&out);
    assert!(text.contains("run   system-probe"), "{text}");
    assert!(
        text.contains("skip  quality-packs (not available"),
        "{text}"
    );
    let json = infermark(home.path(), &["plan", "-p", "quick", "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(v[0]["module_id"], "system-probe");
    assert!(!infermark(home.path(), &["plan", "--profile", "nope"])
        .status
        .success());
}

#[test]
fn run_writes_result_to_out_dir() {
    let home = tempfile::tempdir().unwrap();
    let profile = home.path().join("tiny.toml");
    std::fs::write(
        &profile,
        "schema = 1\nname = \"Tiny\"\n[modules.system-probe]\n[modules.memory-bandwidth]\nsize_mb = 4\niterations = 1\n",
    )
    .unwrap();
    let out_dir = home.path().join("out");
    let out = infermark(
        home.path(),
        &[
            "run",
            "--profile",
            profile.to_str().unwrap(),
            "--out",
            out_dir.to_str().unwrap(),
            "--json",
            "--quiet",
        ],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["profile"]["name"], "Tiny");
    let files: Vec<_> = std::fs::read_dir(&out_dir).unwrap().collect();
    assert_eq!(files.len(), 1);
    // --quiet suppresses progress lines
    assert!(!String::from_utf8_lossy(&out.stderr).contains("[1/2]"));

    // Default output folder is INFERMARK_HOME/results, and progress is shown without --quiet.
    let out = infermark(home.path(), &["run", "-p", profile.to_str().unwrap()]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("[1/2]"));
    assert!(stdout(&out).contains("Triad bandwidth"));
    assert_eq!(
        std::fs::read_dir(home.path().join("results"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn packs_list_and_validate() {
    let home = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_infermark"))
        .args(["packs"])
        .env("INFERMARK_HOME", home.path())
        .env("INFERMARK_PACKS_DIR", fixtures())
        .output()
        .unwrap();
    assert!(stdout(&out).contains("sample-trivia"));
    assert!(String::from_utf8_lossy(&out.stderr).contains("problem"));

    let ok = infermark(
        home.path(),
        &[
            "pack",
            "validate",
            fixtures().join("sample-trivia").to_str().unwrap(),
        ],
    );
    assert!(ok.status.success());
    let bad = infermark(
        home.path(),
        &[
            "pack",
            "validate",
            fixtures().join("broken").to_str().unwrap(),
        ],
    );
    assert!(!bad.status.success());
}

#[test]
fn schema_export_writes_files() {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join("schema");
    assert!(infermark(
        home.path(),
        &["schema", "export", "--dir", dir.to_str().unwrap()]
    )
    .status
    .success());
    assert!(dir.join("result.v1.json").is_file());
    assert!(dir.join("profile.v1.json").is_file());
}
