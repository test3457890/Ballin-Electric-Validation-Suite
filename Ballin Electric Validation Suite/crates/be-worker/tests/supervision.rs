//! Integration tests: real worker process + supervisor, using FIXTURE tests
//! only (no hardware load). Covers clean completion, mismatch, crash,
//! timeout/hang, heartbeat loss, operator stop, protocol violation,
//! simulated over-temperature and sensor loss.

use be_core::inventory::{SensorKind, SensorReading};
use be_core::model::Provenance;
use be_core::status::TestStatus;
use be_core::time::UtcMillis;
use be_safety::policy::HardwareSafetyProfile;
use be_safety::{SensorWatchdog, SupervisorConfig, SupervisorEvent, run_supervised};
use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

fn cfg(timeout_ms: u64) -> SupervisorConfig {
    let mut c = SupervisorConfig::new(
        PathBuf::from(env!("CARGO_BIN_EXE_bevalidate-worker")),
        Duration::from_millis(timeout_ms),
    );
    c.fixture_mode = true;
    c.heartbeat_timeout = Duration::from_secs(2);
    c.cancel_grace = Duration::from_millis(500);
    c
}

fn run(
    c: &SupervisorConfig,
    id: &str,
    params: serde_json::Value,
    cancel: &AtomicBool,
) -> (be_core::model::TestResult, Vec<SupervisorEvent>) {
    let mut ev = Vec::new();
    let r = run_supervised(c, id, &params, cancel, None, &mut |e| ev.push(e));
    (r, ev)
}

#[test]
fn clean_fixture_run_is_labelled_fixture() {
    let (r, ev) = run(
        &cfg(10_000),
        "FIX-T-SLEEP",
        json!({"duration_ms": 200}),
        &AtomicBool::new(false),
    );
    assert_eq!(r.status, TestStatus::Pass, "{r:?}");
    assert_eq!(r.provenance, Provenance::Fixture);
    assert!(
        ev.iter()
            .any(|e| matches!(e, SupervisorEvent::Progress { .. }))
    );
}

#[test]
fn mismatch_reports_fail_with_code() {
    let (r, _) = run(
        &cfg(10_000),
        "FIX-T-MISMATCH",
        json!({}),
        &AtomicBool::new(false),
    );
    assert_eq!(r.status, TestStatus::Fail);
    assert_eq!(r.first_fault_code(), Some("BE-CPU-001"));
}

#[test]
fn crash_is_detected() {
    let (r, _) = run(
        &cfg(10_000),
        "FIX-T-CRASH",
        json!({}),
        &AtomicBool::new(false),
    );
    assert_eq!(r.status, TestStatus::Fail);
    assert_eq!(r.first_fault_code(), Some("BE-SYS-001"));
}

#[test]
fn hang_is_terminated_by_timeout() {
    let t = Instant::now();
    let (r, _) = run(
        &cfg(1_000),
        "FIX-T-HANG",
        json!({}),
        &AtomicBool::new(false),
    );
    assert_eq!(r.status, TestStatus::Aborted);
    assert_eq!(r.first_fault_code(), Some("BE-SYS-002"));
    assert!(t.elapsed() < Duration::from_secs(5));
}

#[cfg(unix)]
#[test]
fn heartbeat_loss_is_detected() {
    let c = cfg(30_000);
    let mut ev = Vec::new();
    let t = Instant::now();
    let r = run_supervised(
        &c,
        "FIX-T-SLEEP",
        &json!({"duration_ms": 20000}),
        &AtomicBool::new(false),
        None,
        &mut |e| {
            if let SupervisorEvent::Started { pid } = e {
                // Freeze the worker: no more heartbeats.
                unsafe { libc_kill(pid as i32, 19) };
            }
            ev.push(e);
        },
    );
    assert_eq!(r.status, TestStatus::Aborted, "{r:?}");
    assert_eq!(r.first_fault_code(), Some("BE-SYS-002"));
    assert!(t.elapsed() < Duration::from_secs(10));
}

#[cfg(unix)]
unsafe extern "C" {
    #[link_name = "kill"]
    fn libc_kill(pid: i32, sig: i32) -> i32;
}

#[test]
fn operator_stop_aborts_cooperatively() {
    let cancel = AtomicBool::new(false);
    let c = cfg(30_000);
    let t = Instant::now();
    let r = std::thread::scope(|s| {
        s.spawn(|| {
            std::thread::sleep(Duration::from_millis(300));
            cancel.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        run_supervised(
            &c,
            "FIX-T-SLEEP",
            &json!({"duration_ms": 20000}),
            &cancel,
            None,
            &mut |_| {},
        )
    });
    assert_eq!(r.status, TestStatus::Aborted);
    assert!(r.faults.iter().any(|f| f.code == "BE-SYS-003"));
    assert!(t.elapsed() < Duration::from_secs(5));
}

#[test]
fn stop_of_uncooperative_worker_forces_kill() {
    let cancel = AtomicBool::new(true);
    let t = Instant::now();
    let (r, _) = run(&cfg(30_000), "FIX-T-HANG", json!({}), &cancel);
    assert_eq!(r.status, TestStatus::Aborted);
    assert!(t.elapsed() < Duration::from_secs(5));
}

#[test]
fn oversized_output_is_bounded() {
    // The worker truncates oversized log lines itself, so the run completes.
    let (r, ev) = run(
        &cfg(10_000),
        "FIX-T-FLOOD",
        json!({"duration_ms": 100}),
        &AtomicBool::new(false),
    );
    assert_eq!(r.status, TestStatus::Pass);
    assert!(ev.iter().any(
        |e| matches!(e, SupervisorEvent::Log { message, .. } if message.ends_with("[truncated]"))
    ));
}

#[test]
fn unknown_test_and_missing_worker() {
    let (r, _) = run(&cfg(5_000), "NOPE", json!({}), &AtomicBool::new(false));
    assert_eq!(r.status, TestStatus::NotRun);
    let mut c = cfg(5_000);
    c.worker_path = PathBuf::from("/nonexistent/worker");
    let (r, _) = run(&c, "FIX-T-SLEEP", json!({}), &AtomicBool::new(false));
    assert_eq!(r.status, TestStatus::NotRun);
    // Fixture tests are invisible to a production-mode worker.
    let mut c = cfg(5_000);
    c.fixture_mode = false;
    let (r, _) = run(&c, "FIX-T-SLEEP", json!({}), &AtomicBool::new(false));
    assert_eq!(r.status, TestStatus::NotRun);
}

#[test]
fn high_load_test_refused_without_authorization() {
    let mut c = cfg(10_000);
    c.fixture_mode = false;
    let (r, _) = run(
        &c,
        "BE-T-CPU-STRESS",
        json!({"duration_s": 1}),
        &AtomicBool::new(false),
    );
    assert_eq!(r.status, TestStatus::NotRun);
    assert_eq!(r.first_fault_code(), Some("BE-SAF-001"));
}

fn profile() -> HardwareSafetyProfile {
    HardwareSafetyProfile::from_json(
        &json!({"schema":1,"id":"fixture","version":"1","description":"FIXTURE","approved":true,"approved_by":"test",
        "sample_interval_ms":50,"max_missed_samples":2,
        "rules":[{"matches":"cpu","kind":"temperature","warn":80.0,"abort":90.0,"required":true}]})
        .to_string(),
    )
    .unwrap()
}

fn reading(v: Option<f64>) -> SensorReading {
    SensorReading {
        id: "fixture:cpu".into(),
        chip: "fixture".into(),
        label: "CPU".into(),
        kind: SensorKind::Temperature,
        value: v,
        unit: "°C".into(),
        crit: None,
        max: None,
        ts: UtcMillis::now(),
        source: "SIMULATED".into(),
    }
}

#[test]
fn simulated_overtemperature_aborts() {
    let mut n = 0;
    let mut wd = SensorWatchdog::new(
        profile(),
        Box::new(move || {
            n += 1;
            vec![reading(Some(if n < 4 {
                60.0
            } else if n < 6 {
                85.0
            } else {
                95.0
            }))]
        }),
    );
    let mut ev = Vec::new();
    let r = run_supervised(
        &cfg(30_000),
        "FIX-T-SLEEP",
        &json!({"duration_ms": 20000}),
        &AtomicBool::new(false),
        Some(&mut wd),
        &mut |e| ev.push(e),
    );
    assert_eq!(r.status, TestStatus::Aborted, "{r:?}");
    assert_eq!(r.first_fault_code(), Some("BE-THERM-002"));
    assert!(
        r.faults.iter().any(|f| f.code == "BE-THERM-001"),
        "warning recorded before abort"
    );
    assert!(
        r.measurements
            .iter()
            .any(|m| m.name.ends_with("max") && m.value == Some(95.0))
    );
}

#[test]
fn simulated_sensor_loss_aborts() {
    let mut n = 0;
    let mut wd = SensorWatchdog::new(
        profile(),
        Box::new(move || {
            n += 1;
            if n < 3 {
                vec![reading(Some(50.0))]
            } else {
                vec![reading(None)]
            }
        }),
    );
    let r = run_supervised(
        &cfg(30_000),
        "FIX-T-SLEEP",
        &json!({"duration_ms": 20000}),
        &AtomicBool::new(false),
        Some(&mut wd),
        &mut |_| {},
    );
    assert_eq!(r.status, TestStatus::Aborted);
    assert_eq!(r.first_fault_code(), Some("BE-SNS-001"));
}

#[test]
fn live_known_answer_test_runs_in_worker() {
    // Low-impact live test (seconds, small buffers): CPU known-answer with
    // two threads and small exponents.
    let mut c = cfg(60_000);
    c.fixture_mode = false;
    let (r, _) = run(
        &c,
        "BE-T-CPU-001",
        json!({"threads": 2, "max_exponent": 607}),
        &AtomicBool::new(false),
    );
    assert_eq!(r.provenance, Provenance::Live);
    assert_eq!(r.status, TestStatus::Pass, "{r:?}");
}

/// GPU tests through the real worker. Needs a Vulkan implementation; CI
/// installs Mesa lavapipe (a SOFTWARE device, so `include_software` is set
/// and results describe the code path, not GPU hardware).
#[test]
fn gpu_tests_run_in_worker_on_available_vulkan_device() {
    let devices = match be_gpu::enumerate() {
        Ok(d) if !d.is_empty() => d,
        other => {
            eprintln!(
                "SKIPPED: no Vulkan device ({:?})",
                other.map(|d| d.len()).map_err(|e| e.to_string())
            );
            return;
        }
    };
    let mut c = cfg(120_000);
    c.fixture_mode = false;
    let (r, _) = run(
        &c,
        "BE-T-GPU-002",
        json!({"elements": 65536, "iterations": 32, "rounds": 2, "include_software": true}),
        &AtomicBool::new(false),
    );
    assert_eq!(r.status, TestStatus::Pass, "{r:?}");
    let (r, _) = run(
        &c,
        "BE-T-GPU-003",
        json!({"size_mib": 16, "include_software": true}),
        &AtomicBool::new(false),
    );
    assert_eq!(r.status, TestStatus::Pass, "{r:?}");
    if devices.iter().all(|d| d.software) {
        // Default selection excludes software devices: never a hardware pass.
        let (r, _) = run(&c, "BE-T-GPU-002", json!({}), &AtomicBool::new(false));
        assert_eq!(r.status, TestStatus::Unsupported);
        assert_eq!(r.first_fault_code(), Some("BE-GPU-002"));
    }
    // High-load graphics stress is refused without authorization.
    let (r, _) = run(
        &c,
        "BE-T-GPU-004",
        json!({"duration_s": 1, "include_software": true}),
        &AtomicBool::new(false),
    );
    assert_eq!(r.status, TestStatus::NotRun);
    let mut auth = cfg(120_000);
    auth.fixture_mode = false;
    auth.high_load_authorized = true;
    let (r, _) = run(
        &auth,
        "BE-T-GPU-004",
        json!({"duration_s": 2, "width": 160, "height": 120, "layers": 4, "frames_per_submit": 2, "verify_every": 2, "include_software": true}),
        &AtomicBool::new(false),
    );
    assert_eq!(r.status, TestStatus::Pass, "{r:?}");
}
