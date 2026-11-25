//! End-to-end engine tests with the real worker binary, using FIXTURE
//! profiles (no hardware load) plus low-impact live steps. Verifies
//! persistence, prerequisite blocking, release gating and stop handling.

use be_core::policy::{ManualCheckRecord, ManualCheckStatus, ReleaseState};
use be_core::profile::Profile;
use be_core::status::TestStatus;
use be_core::time::UtcMillis;
use be_engine::{EngineConfig, EngineEvent, approve_release, evaluate_unit, run_profile};
use be_storage::{Store, Unit};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

fn cfg(fixture: bool) -> EngineConfig {
    let mut c = EngineConfig::new("test-operator");
    c.worker_path = PathBuf::from(env!("CARGO_BIN_EXE_bevalidate-worker"));
    c.fixture_mode = fixture;
    c
}

fn unit(store: &Store, serial: &str) {
    store
        .create_unit(
            &Unit {
                serial: serial.into(),
                chassis_revision: None,
                order_ref: None,
                configuration: None,
                bom_json: None,
                assembly_technician: None,
                created_utc: UtcMillis::now(),
                release_state: ReleaseState::AwaitingTest,
            },
            "op",
        )
        .unwrap();
}

fn fixture_profile() -> Profile {
    Profile::from_json(
        r#"{"schema":1,"id":"fixture-e2e","version":"1","name":"Fixture E2E",
        "steps":[{"test":"FIX-T-SLEEP","params":{"duration_ms":100}},
                 {"test":"FIX-T-MISMATCH","required":true},
                 {"test":"FIX-T-SLEEP","key":"after","requires":["FIX-T-MISMATCH"]}],
        "manual_checks":[{"id":"finish","label":"Finish"}]}"#,
    )
    .unwrap()
}

#[test]
fn fixture_run_persists_and_blocks_release() {
    let store = Store::open_in_memory().unwrap();
    unit(&store, "FIX-1");
    let platform = be_platform::native();
    let mut events = Vec::new();
    let s = run_profile(
        &cfg(true),
        &fixture_profile(),
        Some("FIX-1"),
        None,
        platform.as_ref(),
        Some(&store),
        &AtomicBool::new(false),
        &mut |e| events.push(e),
    )
    .unwrap();
    assert_eq!(s.overall, TestStatus::Fail);
    assert_eq!(s.results[0].1.status, TestStatus::Pass);
    assert_eq!(s.results[1].1.status, TestStatus::Fail);
    assert_eq!(
        s.results[2].1.status,
        TestStatus::NotRun,
        "prerequisite failure blocks dependent step"
    );
    assert_eq!(s.results[2].1.first_fault_code(), Some("BE-SYS-004"));
    let ev = s.evaluation.unwrap();
    assert_eq!(ev.recommended_state, ReleaseState::Hold);
    assert_eq!(
        store.get_unit("FIX-1").unwrap().unwrap().release_state,
        ReleaseState::Hold
    );
    let id = s.run_id.unwrap();
    assert_eq!(store.results_for_run(id).unwrap().len(), 3);
    assert!(!store.events(Some(id), 100).unwrap().is_empty());
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EngineEvent::RunFinished { .. }))
    );
    // Release approval is refused.
    assert!(approve_release(&store, "FIX-1", "qa-lead", "").is_err());
}

#[test]
fn fixture_pass_still_cannot_be_released() {
    let store = Store::open_in_memory().unwrap();
    unit(&store, "FIX-2");
    let p = Profile::from_json(r#"{"schema":1,"id":"f","version":"1","name":"F","steps":[{"test":"FIX-T-SLEEP","params":{"duration_ms":50}}]}"#).unwrap();
    let platform = be_platform::native();
    let s = run_profile(
        &cfg(true),
        &p,
        Some("FIX-2"),
        None,
        platform.as_ref(),
        Some(&store),
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(s.overall, TestStatus::Pass);
    assert!(
        !s.evaluation.unwrap().eligible_for_review,
        "fixture results never qualify"
    );
    let err = approve_release(&store, "FIX-2", "qa-lead", "").unwrap_err();
    assert!(err.blockers[0].contains("FIXTURE"));
}

#[test]
fn live_low_impact_run_with_manual_check_reaches_review_then_release() {
    let store = Store::open_in_memory().unwrap();
    unit(&store, "LIVE-1");
    let p = Profile::from_json(
        r#"{"schema":1,"id":"live-mini","version":"1","name":"Live mini",
        "steps":[{"test":"BE-T-CPU-001","params":{"threads":2,"max_exponent":607},"timeout_s":120},
                 {"test":"BE-T-MEM-001","params":{"size_mib":16},"timeout_s":120}],
        "manual_checks":[{"id":"finish","label":"Finish"}]}"#,
    )
    .unwrap();
    let platform = be_platform::native();
    let s = run_profile(
        &cfg(false),
        &p,
        Some("LIVE-1"),
        None,
        platform.as_ref(),
        Some(&store),
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(s.overall, TestStatus::Pass, "{:?}", s.results);
    let ev = s.evaluation.unwrap();
    assert!(!ev.eligible_for_review, "manual check not yet signed off");
    let run_id = s.run_id.unwrap();
    store
        .add_manual_check(
            "LIVE-1",
            Some(run_id),
            &ManualCheckRecord {
                check_id: "finish".into(),
                status: ManualCheckStatus::Pass,
                operator: "inspector".into(),
                note: String::new(),
            },
        )
        .unwrap();
    let (_, ev) = evaluate_unit(&store, "LIVE-1").unwrap();
    assert!(ev.eligible_for_review, "{:?}", ev.blockers);
    assert!(
        approve_release(&store, "LIVE-1", "", "").is_err(),
        "reviewer identity required"
    );
    approve_release(&store, "LIVE-1", "qa-lead", "bench test").unwrap();
    assert_eq!(
        store.get_unit("LIVE-1").unwrap().unwrap().release_state,
        ReleaseState::Released
    );
}

#[test]
fn high_load_step_refused_by_preflight_blocks_release() {
    let store = Store::open_in_memory().unwrap();
    unit(&store, "HL-1");
    let p = Profile::from_json(r#"{"schema":1,"id":"hl","version":"1","name":"HL","steps":[{"test":"BE-T-CPU-STRESS","params":{"duration_s":1}}]}"#).unwrap();
    let platform = be_platform::native();
    let s = run_profile(
        &cfg(false),
        &p,
        Some("HL-1"),
        None,
        platform.as_ref(),
        Some(&store),
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(s.results[0].1.status, TestStatus::NotRun);
    assert_eq!(s.results[0].1.first_fault_code(), Some("BE-SAF-001"));
    assert!(!s.evaluation.unwrap().eligible_for_review);
}

#[test]
fn stop_marks_remaining_steps_not_run() {
    let cancel = AtomicBool::new(false);
    let p = Profile::from_json(
        r#"{"schema":1,"id":"s","version":"1","name":"S","steps":[{"test":"FIX-T-SLEEP","params":{"duration_ms":20000}},{"test":"FIX-T-SLEEP","key":"second"}]}"#,
    )
    .unwrap();
    let platform = be_platform::native();
    let s = std::thread::scope(|sc| {
        sc.spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(400));
            cancel.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        run_profile(
            &cfg(true),
            &p,
            None,
            None,
            platform.as_ref(),
            None,
            &cancel,
            &mut |_| {},
        )
        .unwrap()
    });
    assert_eq!(s.results[0].1.status, TestStatus::Aborted);
    assert_eq!(s.results[1].1.status, TestStatus::NotRun);
    assert_eq!(s.overall, TestStatus::Aborted);
}
