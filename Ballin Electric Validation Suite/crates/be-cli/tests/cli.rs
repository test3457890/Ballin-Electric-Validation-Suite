//! CLI contract tests: exit codes, JSON output and release gating.
//! Live steps used here are read-only or tiny known-answer checks.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_bevalidate"))
}

fn worker_available() -> bool {
    let name = if cfg!(windows) {
        "bevalidate-worker.exe"
    } else {
        "bevalidate-worker"
    };
    bin()
        .parent()
        .map(|d| d.join(name).exists())
        .unwrap_or(false)
}

fn run(db: &Path, args: &[&str]) -> Output {
    Command::new(bin())
        .arg("--db")
        .arg(db)
        .args(args)
        .output()
        .expect("spawn bevalidate")
}

fn code(o: &Output) -> i32 {
    o.status.code().unwrap_or(-1)
}

#[test]
fn inventory_and_capabilities_json() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("t.db");
    let o = run(&db, &["inventory", "--format", "json"]);
    assert_eq!(code(&o), 0);
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert!(v.get("unavailable").is_some());
    let o = run(&db, &["capabilities", "--format", "json"]);
    assert_eq!(code(&o), 0);
    let caps: Vec<serde_json::Value> = serde_json::from_slice(&o.stdout).unwrap();
    assert!(
        caps.iter()
            .any(|c| c["id"] == "gpu.compute" && c["state"] == "planned")
    );
}

#[test]
fn profile_validation_exit_codes() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("t.db");
    assert_eq!(code(&run(&db, &["profile", "validate", "quick"])), 0);
    let bad = dir.path().join("bad.json");
    std::fs::write(
        &bad,
        r#"{"schema":1,"id":"x","version":"1","name":"x","steps":[{"test":"NO-SUCH-TEST"}]}"#,
    )
    .unwrap();
    assert_eq!(
        code(&run(&db, &["profile", "validate", bad.to_str().unwrap()])),
        4
    );
    assert_eq!(code(&run(&db, &["run", "--profile", "does-not-exist"])), 4);
}

#[test]
fn records_and_release_refusal() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("t.db");
    assert_eq!(code(&run(&db, &["unit", "create", "--serial", "CLI-1"])), 0);
    assert_eq!(
        code(&run(&db, &["unit", "create", "--serial", "CLI-1"])),
        4,
        "duplicate serial"
    );
    assert_eq!(
        code(&run(&db, &["release", "evaluate", "--unit", "CLI-1"])),
        4,
        "no run yet"
    );
    assert_eq!(code(&run(&db, &["report", "--run-id", "999"])), 4);
    assert_eq!(
        code(&run(
            &db,
            &[
                "service",
                "add",
                "--unit",
                "NOPE",
                "--technician",
                "t",
                "--kind",
                "repair",
                "--description",
                "d"
            ]
        )),
        4
    );
}

#[test]
fn fixture_failure_run_exits_fail_and_writes_reports() {
    if !worker_available() {
        eprintln!(
            "SKIPPED: bevalidate-worker not built next to bevalidate (run `cargo test --workspace`)"
        );
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("t.db");
    let prof = dir.path().join("p.json");
    std::fs::write(&prof, r#"{"schema":1,"id":"fx","version":"1","name":"fx","release_profile":true,"steps":[{"test":"FIX-T-MISMATCH"}]}"#).unwrap();
    assert_eq!(code(&run(&db, &["unit", "create", "--serial", "FX-1"])), 0);
    let out = dir.path().join("out");
    let o = run(
        &db,
        &[
            "run",
            "--profile",
            prof.to_str().unwrap(),
            "--unit",
            "FX-1",
            "--fixture-mode",
            "--output",
            out.to_str().unwrap(),
        ],
    );
    assert_eq!(code(&o), 1, "{}", String::from_utf8_lossy(&o.stderr));
    for f in [
        "report.html",
        "report.json",
        "results.csv",
        "measurements.csv",
    ] {
        assert!(out.join(f).exists(), "{f}");
    }
    let html = std::fs::read_to_string(out.join("report.html")).unwrap();
    assert!(html.contains("SIMULATED / FIXTURE DATA"));
    assert_eq!(
        code(&run(
            &db,
            &["release", "approve", "--unit", "FX-1", "--reviewer", "qa"]
        )),
        6
    );
    assert_eq!(
        code(&run(&db, &["release", "evaluate", "--unit", "FX-1"])),
        6
    );
}

#[test]
fn high_load_without_authorization_is_not_run() {
    if !worker_available() {
        eprintln!("SKIPPED: worker not built");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("t.db");
    let prof = dir.path().join("p.json");
    std::fs::write(&prof, r#"{"schema":1,"id":"hl","version":"1","name":"hl","steps":[{"test":"BE-T-CPU-STRESS","params":{"duration_s":1}}]}"#).unwrap();
    let o = run(&db, &["run", "--profile", prof.to_str().unwrap()]);
    assert_eq!(code(&o), 2, "NOT RUN is incomplete, never pass");
    assert!(String::from_utf8_lossy(&o.stderr).contains("BE-SAF-001"));
    // Example safety profile is not approved: still refused even with authorization flag.
    let sp = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../profiles/safety/EXAMPLE-not-approved.json"
    );
    let o = run(
        &db,
        &[
            "run",
            "--profile",
            prof.to_str().unwrap(),
            "--authorize-high-load",
            "--safety-profile",
            sp,
        ],
    );
    assert_eq!(code(&o), 2);
    assert!(String::from_utf8_lossy(&o.stderr).contains("not approved"));
}
