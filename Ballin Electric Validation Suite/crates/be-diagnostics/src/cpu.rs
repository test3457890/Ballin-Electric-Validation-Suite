//! CPU verification workloads: known-answer tests (low impact) and the
//! Prime95-style verified stress workload (high load, authorization required).

use crate::lucas;
use crate::{DiagnosticTest, TestContext, desc, finish, sys};
use be_core::model::*;
use be_core::status::TestStatus;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// NIST FIPS 180-2 SHA-256 test vectors.
const SHA_VECTORS: &[(&[u8], &str)] = &[
    (
        b"",
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    ),
    (
        b"abc",
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    ),
    (
        b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
    ),
];
const SHA_MILLION_A: &str = "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0";

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// SHA-256 known answers. Returns a mismatch description on failure.
pub fn sha_kat(include_million: bool) -> Result<u32, String> {
    let mut n = 0;
    for (msg, want) in SHA_VECTORS {
        let got = hex(&Sha256::digest(msg));
        if got != *want {
            return Err(format!("SHA-256 vector {n}: expected {want}, got {got}"));
        }
        n += 1;
    }
    if include_million {
        let mut h = Sha256::new();
        let block = [b'a'; 1000];
        for _ in 0..1000 {
            h.update(block);
        }
        let got = hex(&h.finalize());
        if got != SHA_MILLION_A {
            return Err(format!(
                "SHA-256 million-'a' vector: expected {SHA_MILLION_A}, got {got}"
            ));
        }
        n += 1;
    }
    Ok(n)
}

/// Exact floating-point matrix multiply check: small-integer matrices whose
/// products are exactly representable in f64 are multiplied with FMA in
/// floating point and compared bit-exactly against an integer reference.
pub fn fp_matmul_kat(n: usize, seed: u64) -> Result<u64, String> {
    // Bijective seed mix; xorshift state must be non-zero.
    let mut s = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0x632B_E59B_D9B4_E019;
    if s == 0 {
        s = 0x632B_E59B_D9B4_E019;
    }
    let mut next = || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        ((s % 17) as i64) - 8
    };
    let a: Vec<i64> = (0..n * n).map(|_| next()).collect();
    let b: Vec<i64> = (0..n * n).map(|_| next()).collect();
    let af: Vec<f64> = a.iter().map(|x| *x as f64).collect();
    let bf: Vec<f64> = b.iter().map(|x| *x as f64).collect();
    let mut checksum = 0u64;
    for i in 0..n {
        for j in 0..n {
            let mut ci: i64 = 0;
            let mut cf: f64 = 0.0;
            for k in 0..n {
                ci += a[i * n + k] * b[k * n + j];
                cf = af[i * n + k].mul_add(bf[k * n + j], cf);
            }
            if cf != ci as f64 {
                return Err(format!(
                    "FP matrix element ({i},{j}): expected {ci}, got {cf}"
                ));
            }
            checksum = checksum.wrapping_mul(31).wrapping_add(ci as u64);
        }
    }
    Ok(checksum)
}

/// One Lucas-Lehmer known-answer check.
pub fn ll_kat(p: u32, cancel: &dyn Fn() -> bool) -> Result<Option<()>, String> {
    let Some(expected) = lucas::expected_prime(p) else {
        return Ok(Some(()));
    };
    match lucas::lucas_lehmer(p, cancel) {
        None => Ok(None),
        Some(got) if got == expected => Ok(Some(())),
        Some(got) => Err(format!(
            "Lucas-Lehmer M{p}: expected {}, computed {}",
            if expected { "prime" } else { "composite" },
            if got { "prime" } else { "composite" }
        )),
    }
}

#[derive(Default)]
struct Shared {
    errors: Mutex<Vec<(usize, String)>>,
    rounds: AtomicU64,
    ll_tests: AtomicU64,
    pinned: AtomicU64,
}

/// Run `round` on one thread per selected logical CPU until `keep_going`
/// returns false (checked between rounds) or an error/cancel occurs.
fn run_threads(
    threads: usize,
    cancel: &AtomicBool,
    stop_on_error: bool,
    shared: &Shared,
    round: &(dyn Fn(usize, u64, &dyn Fn() -> bool) -> Result<u64, String> + Sync),
    keep_going: &(dyn Fn(u64) -> bool + Sync),
) {
    let any_error = AtomicBool::new(false);
    std::thread::scope(|sc| {
        for t in 0..threads {
            let any_error = &any_error;
            sc.spawn(move || {
                if sys::pin_current_thread(t) {
                    shared.pinned.fetch_add(1, Ordering::Relaxed);
                }
                let stop = || {
                    cancel.load(Ordering::Relaxed)
                        || (stop_on_error && any_error.load(Ordering::Relaxed))
                };
                let mut r = 0u64;
                while !stop() && keep_going(r) {
                    match round(t, r, &stop) {
                        Ok(ll) => {
                            shared.ll_tests.fetch_add(ll, Ordering::Relaxed);
                            shared.rounds.fetch_add(1, Ordering::Relaxed);
                        }
                        Err(e) => {
                            shared.errors.lock().unwrap().push((t, e));
                            any_error.store(true, Ordering::Relaxed);
                        }
                    }
                    r += 1;
                }
            });
        }
    });
}

fn thread_count(ctx: &TestContext) -> usize {
    let n = sys::logical_cpus();
    match ctx.params.get("threads") {
        Some(v) if v.as_u64().is_some() => (v.as_u64().unwrap() as usize).clamp(1, n),
        _ => n,
    }
}

pub struct CpuKnownAnswer;

impl DiagnosticTest for CpuKnownAnswer {
    fn descriptor(&self) -> TestDescriptor {
        desc(
            "BE-T-CPU-001",
            "1.0.0",
            "CPU Known-Answer Verification",
            Subsystem::Cpu,
            LoadClass::Low,
            Privilege::Standard,
            10,
            "Runs NIST SHA-256 vectors, Lucas-Lehmer Mersenne residues with known answers, and exact FMA floating-point matrix products on one pinned thread per logical CPU.",
            "Per-thread verification outcomes, vector counts, first mismatch detail",
            "Short duration; detects deterministic computation errors, not marginal instability that appears only under sustained heat/load. Thread pinning available on Linux only.",
            json!({"threads": "all", "max_exponent": 2281, "matrix_n": 64}),
        )
    }

    fn run(&self, ctx: &TestContext) -> TestResult {
        let d = self.descriptor();
        let started = Instant::now();
        let mut r = TestResult::new(&d, "CPU (all logical processors)", ctx.provenance);
        let threads = thread_count(ctx);
        let max_p = ctx
            .p_u64("max_exponent", 2281)
            .min(lucas::KNOWN_LIMIT as u64) as u32;
        let mat_n = ctx.p_u64("matrix_n", 64).clamp(8, 256) as usize;
        let exps: Vec<u32> = [
            89, 107, 127, 131, 521, 523, 607, 1279, 1283, 2203, 2281, 2287, 4253, 4423,
        ]
        .into_iter()
        .filter(|p| *p <= max_p)
        .collect();
        let shared = Shared::default();
        let done = AtomicU64::new(0);
        let round = |t: usize, _r: u64, stop: &dyn Fn() -> bool| -> Result<u64, String> {
            sha_kat(t == 0)?;
            fp_matmul_kat(mat_n, 0xBA11_1E00 + t as u64).and_then(|c| {
                // Every thread must reproduce the reference checksum.
                let reference = fp_matmul_kat(mat_n, 0xBA11_1E00 + t as u64)?;
                if c == reference {
                    Ok(())
                } else {
                    Err("FP matmul checksum not reproducible".into())
                }
            })?;
            let mut n = 0;
            for p in &exps {
                match ll_kat(*p, stop)? {
                    Some(()) => n += 1,
                    None => break,
                }
            }
            let finished = done.fetch_add(1, Ordering::Relaxed) + 1;
            (ctx.progress)(
                Some(finished as f64 / threads as f64),
                "verifying logical processors",
            );
            Ok(n)
        };
        run_threads(threads, ctx.cancel, true, &shared, &round, &|r| r < 1);
        let errors = shared.errors.into_inner().unwrap();
        let pinned = shared.pinned.load(Ordering::Relaxed);
        r.measurements
            .push(Measurement::num("Worker threads", threads as f64, ""));
        r.measurements.push(Measurement::num(
            "Threads pinned to a CPU",
            pinned as f64,
            "",
        ));
        r.measurements.push(Measurement::num(
            "Lucas-Lehmer verifications",
            shared.ll_tests.load(Ordering::Relaxed) as f64,
            "",
        ));
        r.measurements.push(Measurement::num(
            "Largest exponent",
            *exps.last().unwrap_or(&0) as f64,
            "",
        ));
        r.coverage = format!(
            "{threads} logical CPU thread(s){}; SHA-256 (4 NIST vectors on thread 0, 3 on others), LL exponents up to {max_p}, {mat_n}x{mat_n} FMA matrix",
            if pinned as usize == threads {
                ", each pinned"
            } else {
                " (not all pinned)"
            }
        );
        for (t, e) in &errors {
            r.faults.push(
                FaultRecord::new(
                    "BE-CPU-001",
                    format!("Arithmetic mismatch on worker thread {t}"),
                    e.clone(),
                )
                .on(format!("CPU thread {t}")),
            );
        }
        let status = if !errors.is_empty() {
            TestStatus::Fail
        } else if ctx.cancelled() || (shared.rounds.load(Ordering::Relaxed) as usize) < threads {
            TestStatus::Aborted
        } else {
            TestStatus::Pass
        };
        finish(r, started, status)
    }
}

pub struct CpuStress;

/// Exponents for the sustained workload (mix of prime and composite M_p).
const STRESS_EXPONENTS: &[u32] = &[4423, 4421, 9689, 9941, 9949, 11213, 11197];

impl DiagnosticTest for CpuStress {
    fn descriptor(&self) -> TestDescriptor {
        let mut d = desc(
            "BE-T-CPU-STRESS",
            "1.0.0",
            "CPU Verified Stress (Prime95-style)",
            Subsystem::Cpu,
            LoadClass::High,
            Privilege::Standard,
            600,
            "Sustained all-core load: repeated Lucas-Lehmer residues with known answers and FMA matrix products, every result verified. Runs only with operator authorization under safety supervision.",
            "Rounds completed, verification failures with thread id, telemetry recorded by supervisor",
            "Original workload inspired by Prime95 behaviour; not Prime95 and not comparable to its scores. AVX-512 usage depends on compiler/runtime dispatch.",
            json!({"duration_s": 600, "threads": "all", "matrix_n": 192}),
        );
        d.cancel_behavior =
            "Cooperative within ~100 ms; supervisor kills process group on timeout".into();
        d
    }

    fn run(&self, ctx: &TestContext) -> TestResult {
        let d = self.descriptor();
        let started = Instant::now();
        let mut r = TestResult::new(&d, "CPU (all logical processors)", ctx.provenance);
        let threads = thread_count(ctx);
        let duration = Duration::from_secs(ctx.p_u64("duration_s", 600).max(1));
        let mat_n = ctx.p_u64("matrix_n", 192).clamp(8, 512) as usize;
        let shared = Shared::default();
        let deadline = started + duration;
        let round = |t: usize, rnd: u64, stop: &dyn Fn() -> bool| -> Result<u64, String> {
            let p = STRESS_EXPONENTS[(rnd as usize + t) % STRESS_EXPONENTS.len()];
            let n = match ll_kat(p, &|| stop() || Instant::now() >= deadline)? {
                Some(()) => 1,
                None => 0,
            };
            fp_matmul_kat(mat_n, rnd ^ ((t as u64) << 32))?;
            if t == 0 {
                let el = started.elapsed().as_secs_f64();
                (ctx.progress)(
                    Some((el / duration.as_secs_f64()).min(1.0)),
                    "sustained verified load",
                );
            }
            Ok(n)
        };
        run_threads(threads, ctx.cancel, false, &shared, &round, &|_| {
            Instant::now() < deadline
        });
        let errors = shared.errors.into_inner().unwrap();
        r.measurements
            .push(Measurement::num("Worker threads", threads as f64, ""));
        r.measurements.push(Measurement::num(
            "Verified rounds",
            shared.rounds.load(Ordering::Relaxed) as f64,
            "",
        ));
        r.measurements.push(Measurement::num(
            "Lucas-Lehmer verifications",
            shared.ll_tests.load(Ordering::Relaxed) as f64,
            "",
        ));
        r.measurements.push(Measurement::num(
            "Verification errors",
            errors.len() as f64,
            "",
        ));
        r.coverage = format!("{threads} threads for {} s requested", duration.as_secs());
        for (t, e) in errors.iter().take(16) {
            r.faults.push(
                FaultRecord::new(
                    "BE-CPU-001",
                    format!("Verification failure on worker thread {t}"),
                    e.clone(),
                )
                .on(format!("CPU thread {t}")),
            );
        }
        let status = if !errors.is_empty() {
            TestStatus::Fail
        } else if ctx.cancelled() || started.elapsed() + Duration::from_millis(500) < duration {
            TestStatus::Aborted
        } else {
            TestStatus::Pass
        };
        finish(r, started, status)
    }
}

pub struct CombinedLoad;

impl DiagnosticTest for CombinedLoad {
    fn descriptor(&self) -> TestDescriptor {
        desc(
            "BE-T-SYS-001",
            "1.0.0",
            "Combined CPU + Memory Load",
            Subsystem::System,
            LoadClass::High,
            Privilege::Standard,
            900,
            "Concurrent verified CPU workload and memory pattern testing to load power delivery and cooling together. GPU load is not included until a GPU compute backend exists.",
            "CPU verification errors, memory mismatches, supervisor telemetry",
            "Does not load GPUs; cannot be used as evidence of full-system power validation.",
            json!({"duration_s": 900, "memory_mib": 1024}),
        )
    }

    fn run(&self, ctx: &TestContext) -> TestResult {
        let d = self.descriptor();
        let started = Instant::now();
        let mut r = TestResult::new(&d, "System (CPU + RAM)", ctx.provenance);
        let duration = Duration::from_secs(ctx.p_u64("duration_s", 900).max(1));
        let mem_mib = ctx.p_u64("memory_mib", 1024).max(1);
        let threads = sys::logical_cpus().saturating_sub(1).max(1);
        let deadline = started + duration;
        let shared = Shared::default();
        let mem_result = std::thread::scope(|sc| {
            let mem = sc.spawn(|| crate::memory::pattern_loop(mem_mib, ctx, deadline));
            let round = |t: usize, rnd: u64, stop: &dyn Fn() -> bool| -> Result<u64, String> {
                let p = STRESS_EXPONENTS[(rnd as usize + t) % STRESS_EXPONENTS.len()];
                let n = ll_kat(p, &|| stop() || Instant::now() >= deadline)?
                    .map(|_| 1)
                    .unwrap_or(0);
                fp_matmul_kat(128, rnd)?;
                if t == 0 {
                    (ctx.progress)(
                        Some((started.elapsed().as_secs_f64() / duration.as_secs_f64()).min(1.0)),
                        "combined load",
                    );
                }
                Ok(n)
            };
            run_threads(threads, ctx.cancel, false, &shared, &round, &|_| {
                Instant::now() < deadline
            });
            mem.join()
                .unwrap_or_else(|_| Err("memory worker panicked".into()))
        });
        let errors = shared.errors.into_inner().unwrap();
        for (t, e) in errors.iter().take(16) {
            r.faults.push(FaultRecord::new(
                "BE-CPU-001",
                format!("Verification failure on CPU thread {t}"),
                e.clone(),
            ));
        }
        let mem_stats = match mem_result {
            Ok(s) => s,
            Err(e) => {
                r.notes.push(e);
                Default::default()
            }
        };
        if mem_stats.mismatches > 0 {
            r.faults.push(FaultRecord::new(
                "BE-MEM-001",
                "Memory mismatch during combined load",
                format!("{} mismatched words", mem_stats.mismatches),
            ));
        }
        r.measurements
            .push(Measurement::num("CPU threads", threads as f64, ""));
        r.measurements.push(Measurement::num(
            "Verified CPU rounds",
            shared.rounds.load(Ordering::Relaxed) as f64,
            "",
        ));
        r.measurements.push(Measurement::num(
            "Memory buffer",
            mem_stats.buffer_mib as f64,
            "MiB",
        ));
        r.measurements.push(Measurement::num(
            "Memory passes",
            mem_stats.passes as f64,
            "",
        ));
        r.coverage = format!(
            "CPU {threads} threads + {} MiB memory patterns; GPUs NOT loaded",
            mem_stats.buffer_mib
        );
        let status = if !r.faults.is_empty() {
            TestStatus::Fail
        } else if ctx.cancelled()
            || started.elapsed() + Duration::from_millis(500) < duration
            || mem_stats.passes == 0
        {
            TestStatus::Aborted
        } else {
            TestStatus::Pass
        };
        finish(r, started, status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha_vectors_pass() {
        assert_eq!(sha_kat(true), Ok(4));
    }

    #[test]
    fn fp_matmul_is_exact_and_deterministic() {
        let a = fp_matmul_kat(48, 42).unwrap();
        assert_eq!(a, fp_matmul_kat(48, 42).unwrap());
        assert_ne!(a, fp_matmul_kat(48, 43).unwrap());
    }

    #[test]
    fn ll_kat_detects_expected() {
        assert_eq!(ll_kat(127, &|| false), Ok(Some(())));
        assert_eq!(ll_kat(131, &|| false), Ok(Some(())));
        assert_eq!(ll_kat(4423, &|| true), Ok(None));
    }

    #[test]
    fn stress_exponents_have_known_answers() {
        for p in STRESS_EXPONENTS {
            assert!(lucas::expected_prime(*p).is_some(), "{p}");
        }
    }
}
