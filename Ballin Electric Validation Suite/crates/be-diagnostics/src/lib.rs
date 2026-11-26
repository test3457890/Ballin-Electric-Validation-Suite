//! Diagnostic test modules for BE Validation.
//!
//! Every module declares a typed [`TestDescriptor`] and produces a typed
//! [`TestResult`]. Tests never fabricate readings: unavailable data yields
//! `Unsupported`/`Inconclusive`, never `Pass`.

use be_core::model::{EventLevel, LoadClass, Provenance, TestDescriptor, TestResult};
use be_core::status::TestStatus;
use be_platform::PlatformProvider;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

pub mod cpu;
pub mod fixtures;
pub mod gpu;
pub mod lucas;
pub mod memory;
pub mod memtest;
pub mod reviews;
pub mod storage;
pub mod sys;

/// Execution context handed to a test by the worker.
pub struct TestContext<'a> {
    pub params: &'a serde_json::Value,
    pub cancel: &'a AtomicBool,
    pub progress: &'a (dyn Fn(Option<f64>, &str) + Sync),
    pub log: &'a (dyn Fn(EventLevel, Option<&str>, &str) + Sync),
    pub provenance: Provenance,
    pub platform: &'a dyn PlatformProvider,
    /// Set by the supervisor only after authorization and safety checks.
    pub high_load_authorized: bool,
}

impl TestContext<'_> {
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
    pub fn p_u64(&self, key: &str, default: u64) -> u64 {
        self.params
            .get(key)
            .and_then(|v| v.as_u64())
            .unwrap_or(default)
    }
    pub fn p_f64(&self, key: &str, default: f64) -> f64 {
        self.params
            .get(key)
            .and_then(|v| v.as_f64())
            .unwrap_or(default)
    }
    pub fn p_bool(&self, key: &str, default: bool) -> bool {
        self.params
            .get(key)
            .and_then(|v| v.as_bool())
            .unwrap_or(default)
    }
    pub fn p_str(&self, key: &str) -> Option<&str> {
        self.params.get(key).and_then(|v| v.as_str())
    }
}

pub trait DiagnosticTest: Send + Sync {
    fn descriptor(&self) -> TestDescriptor;
    fn run(&self, ctx: &TestContext) -> TestResult;
}

/// All production test modules.
pub fn registry() -> Vec<Box<dyn DiagnosticTest>> {
    vec![
        Box::new(reviews::InventoryReview),
        Box::new(reviews::SensorSurvey),
        Box::new(cpu::CpuKnownAnswer),
        Box::new(memory::MemoryPattern),
        Box::new(storage::ScratchIntegrity),
        Box::new(storage::StorageHealth),
        Box::new(reviews::HardwareErrorLog),
        Box::new(reviews::PcieLinkReview),
        Box::new(reviews::NetworkLinkReview),
        Box::new(reviews::FirmwareSecurityReview),
        Box::new(gpu::GpuInventory),
        Box::new(gpu::GpuCompute),
        Box::new(gpu::GpuVram),
        Box::new(gpu::GpuGraphics),
        Box::new(cpu::CpuStress),
        Box::new(memory::MemoryStress),
        Box::new(cpu::CombinedLoad),
    ]
}

/// Look up a test. Fixture tests are only visible when `fixture_mode` is set.
pub fn find(id: &str, fixture_mode: bool) -> Option<Box<dyn DiagnosticTest>> {
    let mut all = registry();
    if fixture_mode {
        all.extend(fixtures::fixture_registry());
    }
    all.into_iter().find(|t| t.descriptor().id == id)
}

pub fn descriptors(fixture_mode: bool) -> Vec<TestDescriptor> {
    let mut all = registry();
    if fixture_mode {
        all.extend(fixtures::fixture_registry());
    }
    all.iter().map(|t| t.descriptor()).collect()
}

/// Run a test with the safety pre-check that high-load tests require
/// authorization. Called by the worker.
pub fn execute(test: &dyn DiagnosticTest, ctx: &TestContext) -> TestResult {
    let d = test.descriptor();
    if d.load == LoadClass::High && !ctx.high_load_authorized {
        let mut r = TestResult::not_executed(
            &d.id,
            &d.version,
            TestStatus::NotRun,
            "High-load test requires explicit operator authorization and an approved hardware safety profile",
            ctx.provenance,
        );
        r.faults.push(be_core::model::FaultRecord::new(
            "BE-SAF-001",
            "High-load test not authorized",
            "Authorization flag not set by supervisor",
        ));
        return r;
    }
    let started = Instant::now();
    let mut r = test.run(ctx);
    if r.duration_ms == 0 {
        r.duration_ms = started.elapsed().as_millis() as u64;
    }
    if ctx.cancelled() && r.status == TestStatus::Pass {
        // A cancelled run is never a pass.
        r.status = TestStatus::Aborted;
        r.notes.push("Cancelled before completion".into());
    }
    r
}

/// Helper used by tests to finish a result with timing.
pub(crate) fn finish(mut r: TestResult, started: Instant, status: TestStatus) -> TestResult {
    r.status = status;
    r.duration_ms = started.elapsed().as_millis() as u64;
    r
}

/// Standard descriptor builder.
#[allow(clippy::too_many_arguments)]
pub(crate) fn desc(
    id: &str,
    version: &str,
    name: &str,
    subsystem: be_core::model::Subsystem,
    load: LoadClass,
    privilege: be_core::model::Privilege,
    typical_duration_s: u64,
    description: &str,
    evidence: &str,
    limitations: &str,
    default_params: serde_json::Value,
) -> TestDescriptor {
    TestDescriptor {
        id: id.into(),
        version: version.into(),
        name: name.into(),
        subsystem,
        description: description.into(),
        load,
        privilege,
        destructive: false,
        typical_duration_s,
        cancel_behavior: "Cooperative: polled between work units; supervisor terminates the worker process on timeout".into(),
        evidence: evidence.into(),
        limitations: limitations.into(),
        manual_steps: Vec::new(),
        default_params,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn registry_ids_unique_and_versioned() {
        let mut ids = HashSet::new();
        for d in descriptors(true) {
            assert!(ids.insert(d.id.clone()), "duplicate {}", d.id);
            assert!(!d.version.is_empty());
            assert!(
                !d.destructive,
                "{} must not be destructive in initial release",
                d.id
            );
            assert!(d.default_params.is_object(), "{}", d.id);
        }
    }

    #[test]
    fn fixture_tests_hidden_in_production() {
        assert!(find("FIX-T-SLEEP", false).is_none());
        assert!(find("FIX-T-SLEEP", true).is_some());
        assert!(descriptors(false).iter().all(|d| !d.id.starts_with("FIX-")));
    }
}
