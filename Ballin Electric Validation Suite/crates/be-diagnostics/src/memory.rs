//! System RAM tests over allocated buffers.

use crate::memtest::{self, Pattern};
use crate::{DiagnosticTest, TestContext, desc, finish};
use be_core::model::*;
use be_core::status::TestStatus;
use serde_json::json;
use std::time::Instant;

const MIB_WORDS: usize = 1024 * 1024 / 8;

#[derive(Default, Debug)]
pub struct LoopStats {
    pub buffer_mib: u64,
    pub passes: u64,
    pub mismatches: u64,
    pub first: Vec<memtest::Mismatch>,
}

/// Repeat the standard pattern set until `deadline` (used by combined load).
pub fn pattern_loop(mib: u64, ctx: &TestContext, deadline: Instant) -> Result<LoopStats, String> {
    let mut buf = memtest::allocate(mib as usize * MIB_WORDS, MIB_WORDS)
        .ok_or("could not allocate memory test buffer")?;
    let mut st = LoopStats {
        buffer_mib: (buf.len() / MIB_WORDS) as u64,
        ..Default::default()
    };
    let mut pass = 0;
    'outer: while Instant::now() < deadline && !ctx.cancelled() {
        for p in Pattern::standard_set(pass) {
            let o = memtest::run_pattern(
                &mut buf,
                p,
                &|| ctx.cancelled() || Instant::now() >= deadline,
                &mut |_| {},
            );
            st.mismatches += o.mismatch_count;
            if st.first.len() < 8 {
                st.first.extend(o.mismatches.into_iter().take(8));
            }
            if o.cancelled {
                break 'outer;
            }
        }
        pass += 1;
        st.passes += 1;
    }
    Ok(st)
}

fn run_memory(d: TestDescriptor, ctx: &TestContext, requested_mib: u64, passes: u64) -> TestResult {
    let started = Instant::now();
    let mut r = TestResult::new(&d, "System RAM (allocated buffer)", ctx.provenance);
    let avail = ctx.platform.inventory_memory_available();
    // Never take more than 50% of currently available memory.
    let cap_mib = avail.map(|a| a / 2 / (1024 * 1024)).unwrap_or(256);
    let mib = requested_mib.min(cap_mib).max(1);
    if mib < requested_mib {
        r.notes.push(format!(
            "Requested {requested_mib} MiB reduced to {mib} MiB (50% of available memory cap)"
        ));
    }
    let Some(mut buf) = memtest::allocate(mib as usize * MIB_WORDS, MIB_WORDS) else {
        r.faults.push(FaultRecord::new(
            "BE-MEM-002",
            "Memory test buffer allocation failed",
            format!("Could not allocate even 1 MiB (requested {mib} MiB)"),
        ));
        return finish(r, started, TestStatus::Inconclusive);
    };
    let got_mib = (buf.len() / MIB_WORDS) as u64;
    if got_mib < mib {
        r.faults.push(FaultRecord::new(
            "BE-MEM-002",
            "Memory test coverage reduced",
            format!("Allocated {got_mib} MiB of {mib} MiB requested"),
        ));
    }
    let patterns_per_pass = Pattern::standard_set(0).len() as u64;
    let total = passes * patterns_per_pass;
    let mut step = 0u64;
    let mut mismatch_total = 0u64;
    let mut words = 0u64;
    let mut first = Vec::new();
    'outer: for pass in 0..passes {
        for p in Pattern::standard_set(pass) {
            (ctx.progress)(
                Some(step as f64 / total as f64),
                &format!("pass {} / {passes}: {}", pass + 1, p.name()),
            );
            let o = memtest::run_pattern(&mut buf, p, &|| ctx.cancelled(), &mut |_| {});
            words += o.words_verified;
            if o.mismatch_count > 0 {
                (ctx.log)(
                    EventLevel::Error,
                    Some("BE-MEM-001"),
                    &format!("{} mismatches in pattern {}", o.mismatch_count, p.name()),
                );
            }
            mismatch_total += o.mismatch_count;
            for m in o.mismatches.iter().take(8) {
                first.push((p.name(), m.clone()));
            }
            if o.cancelled {
                break 'outer;
            }
            step += 1;
        }
    }
    r.measurements
        .push(Measurement::num("Buffer size", got_mib as f64, "MiB"));
    r.measurements
        .push(Measurement::num("Patterns completed", step as f64, ""));
    r.measurements
        .push(Measurement::num("Words verified", words as f64, ""));
    r.measurements.push(Measurement::num(
        "Mismatched words",
        mismatch_total as f64,
        "",
    ));
    r.coverage = format!(
        "{got_mib} MiB virtual buffer, {step}/{total} pattern runs (zeros, ones, checkerboard, walking 1/0, own-address, random, moving inversions); OS-reserved memory not covered"
    );
    for (pname, m) in first.iter().take(16) {
        r.faults.push(FaultRecord::new(
            "BE-MEM-001",
            format!("Pattern mismatch ({pname})"),
            format!(
                "word offset {:#x}: expected {:#018x}, read {:#018x} ({} bit(s) differ)",
                m.word_index * 8,
                m.expected,
                m.actual,
                m.bit_flips()
            ),
        ));
    }
    let status = if mismatch_total > 0 {
        TestStatus::Fail
    } else if step < total {
        TestStatus::Aborted
    } else if got_mib < mib {
        TestStatus::Warning
    } else {
        TestStatus::Pass
    };
    finish(r, started, status)
}

pub struct MemoryPattern;

impl DiagnosticTest for MemoryPattern {
    fn descriptor(&self) -> TestDescriptor {
        desc(
            "BE-T-MEM-001",
            "1.0.0",
            "System Memory Pattern Test",
            Subsystem::Memory,
            LoadClass::Low,
            Privilege::Standard,
            20,
            "Writes and verifies eight address/data patterns in an allocated buffer using volatile accesses.",
            "Mismatch offsets, expected/actual words, bit-flip counts",
            "Covers only the allocated virtual buffer (default 256 MiB, capped at 50% of available). Use preboot diagnostics for full physical coverage.",
            json!({"size_mib": 256, "passes": 1}),
        )
    }
    fn run(&self, ctx: &TestContext) -> TestResult {
        run_memory(
            self.descriptor(),
            ctx,
            ctx.p_u64("size_mib", 256),
            ctx.p_u64("passes", 1).max(1),
        )
    }
}

pub struct MemoryStress;

impl DiagnosticTest for MemoryStress {
    fn descriptor(&self) -> TestDescriptor {
        desc(
            "BE-T-MEM-STRESS",
            "1.0.0",
            "System Memory Extended Integrity",
            Subsystem::Memory,
            LoadClass::High,
            Privilege::Standard,
            1800,
            "Multiple passes of all patterns over a large buffer (up to 50% of available memory).",
            "Mismatch offsets and counts per pass",
            "OS-based; cannot cover kernel-reserved memory. Large buffers may cause swapping on systems with swap enabled.",
            json!({"size_mib": 8192, "passes": 4}),
        )
    }
    fn run(&self, ctx: &TestContext) -> TestResult {
        run_memory(
            self.descriptor(),
            ctx,
            ctx.p_u64("size_mib", 8192),
            ctx.p_u64("passes", 4).max(1),
        )
    }
}

/// Small extension so tests can ask for available memory cheaply.
pub trait MemAvail {
    fn inventory_memory_available(&self) -> Option<u64>;
}

impl<T: be_platform::PlatformProvider + ?Sized> MemAvail for T {
    fn inventory_memory_available(&self) -> Option<u64> {
        available_memory()
    }
}

/// Available memory from the OS (Linux: MemAvailable).
pub fn available_memory() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let t = std::fs::read_to_string("/proc/meminfo").ok()?;
        t.lines()
            .find(|l| l.starts_with("MemAvailable:"))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse::<u64>().ok())
            .map(|k| k * 1024)
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}
