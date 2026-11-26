//! DEVELOPMENT FIXTURE TESTS. Only available when the worker is started in
//! fixture mode; results are labelled `Provenance::Fixture` and can never
//! satisfy release criteria. Used to test supervision (crash, hang, timeout,
//! mismatch, cancellation) without touching hardware.

use crate::{DiagnosticTest, TestContext, desc, finish};
use be_core::model::*;
use be_core::status::TestStatus;
use serde_json::json;
use std::time::{Duration, Instant};

pub fn fixture_registry() -> Vec<Box<dyn DiagnosticTest>> {
    vec![
        Box::new(Sleep),
        Box::new(Crash),
        Box::new(Hang),
        Box::new(Mismatch),
        Box::new(Flood),
    ]
}

fn fdesc(id: &str, name: &str) -> TestDescriptor {
    desc(
        id,
        "1.0.0",
        name,
        Subsystem::System,
        LoadClass::ReadOnly,
        Privilege::Standard,
        1,
        "Development fixture (simulated behaviour; no hardware access)",
        "Simulated",
        "FIXTURE ONLY",
        json!({}),
    )
}

pub struct Sleep;
impl DiagnosticTest for Sleep {
    fn descriptor(&self) -> TestDescriptor {
        fdesc("FIX-T-SLEEP", "[FIXTURE] Sleep with heartbeats")
    }
    fn run(&self, ctx: &TestContext) -> TestResult {
        let started = Instant::now();
        let mut r = TestResult::new(&self.descriptor(), "fixture", Provenance::Fixture);
        let total = ctx.p_u64("duration_ms", 300);
        let mut el = 0;
        while el < total {
            if ctx.cancelled() {
                return finish(r, started, TestStatus::Aborted);
            }
            std::thread::sleep(Duration::from_millis(20));
            el += 20;
            (ctx.progress)(Some(el as f64 / total as f64), "sleeping");
        }
        r.coverage = "simulated".into();
        r.measurements
            .push(Measurement::num("Simulated duration", total as f64, "ms"));
        finish(r, started, TestStatus::Pass)
    }
}

pub struct Crash;
impl DiagnosticTest for Crash {
    fn descriptor(&self) -> TestDescriptor {
        fdesc("FIX-T-CRASH", "[FIXTURE] Worker crash")
    }
    fn run(&self, _ctx: &TestContext) -> TestResult {
        std::process::abort();
    }
}

pub struct Hang;
impl DiagnosticTest for Hang {
    fn descriptor(&self) -> TestDescriptor {
        fdesc("FIX-T-HANG", "[FIXTURE] Hang without heartbeat")
    }
    fn run(&self, _ctx: &TestContext) -> TestResult {
        // Ignores cancellation deliberately to exercise forced termination.
        loop {
            std::thread::sleep(Duration::from_secs(3600));
        }
    }
}

pub struct Mismatch;
impl DiagnosticTest for Mismatch {
    fn descriptor(&self) -> TestDescriptor {
        fdesc("FIX-T-MISMATCH", "[FIXTURE] Simulated arithmetic mismatch")
    }
    fn run(&self, _ctx: &TestContext) -> TestResult {
        let started = Instant::now();
        let mut r = TestResult::new(&self.descriptor(), "fixture", Provenance::Fixture);
        r.faults.push(FaultRecord::new(
            "BE-CPU-001",
            "[SIMULATED] arithmetic mismatch",
            "fixture-injected",
        ));
        finish(r, started, TestStatus::Fail)
    }
}

pub struct Flood;
impl DiagnosticTest for Flood {
    fn descriptor(&self) -> TestDescriptor {
        fdesc("FIX-T-FLOOD", "[FIXTURE] Oversized protocol output")
    }
    fn run(&self, ctx: &TestContext) -> TestResult {
        let big = "x".repeat(be_core::ipc::MAX_LINE_BYTES + 10);
        (ctx.log)(EventLevel::Info, None, &big);
        let mut result = Sleep.run(ctx);
        result.test_id = self.descriptor().id;
        result
    }
}
