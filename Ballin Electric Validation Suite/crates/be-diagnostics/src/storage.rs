//! Storage tests: scratch-file integrity (dedicated temp file only; never
//! raw device writes) and read-only health review.

use crate::{DiagnosticTest, TestContext, desc, finish, sys};
use be_core::model::*;
use be_core::status::TestStatus;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

const BLOCK: usize = 1024 * 1024;

fn fill_block(buf: &mut [u8], seed: u64, block: u64) {
    let mut s = seed ^ block.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    for chunk in buf.chunks_exact_mut(8) {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        chunk.copy_from_slice(&s.to_le_bytes());
    }
}

/// Removes the scratch file on every exit path.
struct ScratchGuard(PathBuf);
impl Drop for ScratchGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub struct ScratchIntegrity;

impl ScratchIntegrity {
    /// Core routine, also used by fixture tests with a `corrupt` hook that
    /// runs after writing and before verification.
    pub fn run_in(ctx: &TestContext, dir: &Path, mib: u64, corrupt: &dyn Fn(&Path)) -> TestResult {
        let d = ScratchIntegrity.descriptor();
        let started = Instant::now();
        let mut r = TestResult::new(
            &d,
            format!("Volume containing {}", dir.display()),
            ctx.provenance,
        );
        let need = mib * BLOCK as u64;
        match sys::free_space(dir) {
            Some(free) => {
                r.measurements.push(Measurement::num(
                    "Free space before test",
                    (free / (1024 * 1024)) as f64,
                    "MiB",
                ));
                // Keep at least 1 GiB and 10% headroom.
                if free < need + (1 << 30) || need > free / 10 * 9 {
                    r.faults.push(FaultRecord::new(
                        "BE-STO-003",
                        "Insufficient scratch space",
                        format!("{} MiB free, {mib} MiB requested", free / (1024 * 1024)),
                    ));
                    return finish(r, started, TestStatus::NotRun);
                }
            }
            None if mib > 256 => {
                r.notes
                    .push("Free space unknown on this platform; size limited to 256 MiB".into());
                return finish(r, started, TestStatus::NotRun);
            }
            None => r
                .measurements
                .push(Measurement::unavailable("Free space before test", "MiB")),
        }
        let path = dir.join(format!(
            "be-validation-scratch-{}-{}.bin",
            std::process::id(),
            started.elapsed().as_nanos()
        ));
        let _guard = ScratchGuard(path.clone());
        let seed = 0xBE01_5C7A_7C4E_u64;
        let mut buf = vec![0u8; BLOCK];
        let mut wh = Sha256::new();
        let t_write = Instant::now();
        let write = (|| -> std::io::Result<()> {
            let mut f = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            for b in 0..mib {
                if ctx.cancelled() {
                    return Err(std::io::Error::other("cancelled"));
                }
                fill_block(&mut buf, seed, b);
                wh.update(&buf);
                f.write_all(&buf)?;
                (ctx.progress)(Some(b as f64 / (2 * mib) as f64), "writing scratch file");
            }
            f.sync_all()?;
            sys::drop_file_cache(&f);
            Ok(())
        })();
        let write_s = t_write.elapsed().as_secs_f64();
        if let Err(e) = write {
            r.notes.push(format!("Write phase stopped: {e}"));
            let st = if ctx.cancelled() {
                TestStatus::Aborted
            } else {
                TestStatus::Fail
            };
            if st == TestStatus::Fail {
                r.faults.push(FaultRecord::new(
                    "BE-STO-001",
                    "Scratch file write error",
                    e.to_string(),
                ));
            }
            return finish(r, started, st);
        }
        corrupt(&path);
        let t_read = Instant::now();
        let mut rh = Sha256::new();
        let mut bad_blocks = Vec::new();
        let mut expected = vec![0u8; BLOCK];
        let read = (|| -> std::io::Result<()> {
            let mut f = File::open(&path)?;
            sys::drop_file_cache(&f);
            for b in 0..mib {
                if ctx.cancelled() {
                    return Err(std::io::Error::other("cancelled"));
                }
                f.read_exact(&mut buf)?;
                fill_block(&mut expected, seed, b);
                rh.update(&buf);
                if buf != expected {
                    let off = buf
                        .iter()
                        .zip(&expected)
                        .position(|(a, e)| a != e)
                        .unwrap_or(0);
                    bad_blocks.push((b, off));
                }
                (ctx.progress)(
                    Some(0.5 + b as f64 / (2 * mib) as f64),
                    "verifying scratch file",
                );
            }
            Ok(())
        })();
        let read_s = t_read.elapsed().as_secs_f64();
        if let Err(e) = read {
            r.notes.push(format!("Read phase stopped: {e}"));
            let st = if ctx.cancelled() {
                TestStatus::Aborted
            } else {
                TestStatus::Fail
            };
            if st == TestStatus::Fail {
                r.faults.push(FaultRecord::new(
                    "BE-STO-001",
                    "Scratch file read error",
                    e.to_string(),
                ));
            }
            return finish(r, started, st);
        }
        let wd = wh.finalize();
        let rd = rh.finalize();
        r.measurements
            .push(Measurement::num("Scratch size", mib as f64, "MiB"));
        r.measurements.push(Measurement::derived(
            "Write throughput (incl. fsync)",
            mib as f64 / write_s.max(1e-9),
            "MiB/s",
        ));
        r.measurements.push(Measurement::derived(
            "Read throughput",
            mib as f64 / read_s.max(1e-9),
            "MiB/s",
        ));
        r.measurements.push(Measurement::text(
            "SHA-256 written",
            wd.iter().map(|b| format!("{b:02x}")).collect::<String>(),
        ));
        r.coverage = format!(
            "{mib} MiB dedicated scratch file, block-level compare and SHA-256; cache eviction is advisory so reads may be served from OS cache (not a media scan)"
        );
        for (b, off) in bad_blocks.iter().take(16) {
            r.faults.push(FaultRecord::new(
                "BE-STO-001",
                "Scratch data mismatch",
                format!("block {b} first differing byte at offset {off}"),
            ));
        }
        let status = if !bad_blocks.is_empty() || wd != rd {
            TestStatus::Fail
        } else {
            TestStatus::Pass
        };
        finish(r, started, status)
    }
}

impl DiagnosticTest for ScratchIntegrity {
    fn descriptor(&self) -> TestDescriptor {
        desc(
            "BE-T-STO-001",
            "1.0.0",
            "Storage Scratch-File Integrity",
            Subsystem::Storage,
            LoadClass::Low,
            Privilege::Standard,
            15,
            "Writes a deterministic pseudo-random scratch file in a dedicated temporary location, syncs, evicts cache where supported, reads back and verifies every block. The file is always deleted.",
            "Block mismatch locations, SHA-256 of written data, measured throughput",
            "Never writes to raw devices or user files. Read-back may be served from OS cache. Throughput is informational, not a benchmark.",
            json!({"size_mib": 128, "directory": null}),
        )
    }

    fn run(&self, ctx: &TestContext) -> TestResult {
        let dir = ctx
            .p_str("directory")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        // Profile-supplied directories must be existing, absolute, real
        // directories (not symlinks) to avoid writing to unexpected places.
        let ok = dir.is_absolute()
            && std::fs::symlink_metadata(&dir)
                .map(|m| m.is_dir())
                .unwrap_or(false);
        if !ok {
            let d = self.descriptor();
            let mut r = TestResult::not_executed(
                &d.id,
                &d.version,
                TestStatus::NotRun,
                format!(
                    "Scratch directory {} is not an existing absolute directory",
                    dir.display()
                ),
                ctx.provenance,
            );
            r.faults.push(FaultRecord::new(
                "BE-STO-003",
                "Invalid scratch directory",
                dir.display().to_string(),
            ));
            return r;
        }
        let mib = ctx.p_u64("size_mib", 128).clamp(1, 16 * 1024);
        Self::run_in(ctx, &dir, mib, &|_| {})
    }
}

/// Parse smartctl JSON (`smartctl -j -H -A`) into measurements and faults.
/// Returns (status, measurements, faults).
pub fn evaluate_smartctl(dev: &str, v: &Value) -> (TestStatus, Vec<Measurement>, Vec<FaultRecord>) {
    let mut m = Vec::new();
    let mut f = Vec::new();
    let mut status = TestStatus::Pass;
    match v.pointer("/smart_status/passed").and_then(|b| b.as_bool()) {
        Some(true) => m.push(Measurement::text(format!("{dev} SMART overall"), "PASSED")),
        Some(false) => {
            m.push(Measurement::text(format!("{dev} SMART overall"), "FAILED"));
            f.push(
                FaultRecord::new(
                    "BE-STO-002",
                    "SMART self-assessment FAILED",
                    "smart_status.passed = false",
                )
                .on(dev),
            );
            status = TestStatus::Fail;
        }
        None => {
            m.push(Measurement::unavailable(format!("{dev} SMART overall"), ""));
            status = TestStatus::Inconclusive;
        }
    }
    if let Some(log) = v.get("nvme_smart_health_information_log") {
        let get = |k: &str| log.get(k).and_then(|x| x.as_f64());
        if let Some(cw) = get("critical_warning") {
            m.push(Measurement::num(
                format!("{dev} critical warning"),
                cw,
                "bitmask",
            ));
            if cw != 0.0 {
                f.push(
                    FaultRecord::new(
                        "BE-STO-002",
                        "NVMe critical warning set",
                        format!("critical_warning = {cw}"),
                    )
                    .on(dev),
                );
                status = TestStatus::Fail;
            }
        }
        for (k, label, unit) in [
            ("percentage_used", "endurance used", "%"),
            ("available_spare", "available spare", "%"),
            ("temperature", "temperature", "°C"),
            ("media_errors", "media errors", ""),
            ("num_err_log_entries", "error log entries", ""),
            ("unsafe_shutdowns", "unsafe shutdowns", ""),
            ("power_on_hours", "power-on hours", "h"),
        ] {
            match get(k) {
                Some(x) => m.push(Measurement::num(format!("{dev} {label}"), x, unit)),
                None => m.push(Measurement::unavailable(format!("{dev} {label}"), unit)),
            }
        }
        if get("media_errors").unwrap_or(0.0) > 0.0 {
            f.push(
                FaultRecord::new(
                    "BE-STO-004",
                    "NVMe media errors recorded",
                    format!("media_errors = {}", get("media_errors").unwrap()),
                )
                .on(dev),
            );
            if status == TestStatus::Pass {
                status = TestStatus::Warning;
            }
        }
    }
    if let Some(table) = v
        .pointer("/ata_smart_attributes/table")
        .and_then(|t| t.as_array())
    {
        for a in table {
            let id = a.get("id").and_then(|x| x.as_u64()).unwrap_or(0);
            let raw = a.pointer("/raw/value").and_then(|x| x.as_f64());
            let name = a.get("name").and_then(|x| x.as_str()).unwrap_or("attr");
            if matches!(id, 5 | 187 | 197 | 198)
                && let Some(raw) = raw
            {
                m.push(Measurement::num(format!("{dev} {name}"), raw, ""));
                if raw > 0.0 {
                    f.push(
                        FaultRecord::new(
                            "BE-STO-004",
                            format!("{name} non-zero"),
                            format!("attribute {id} raw = {raw}"),
                        )
                        .on(dev),
                    );
                    if status == TestStatus::Pass {
                        status = TestStatus::Warning;
                    }
                }
            }
        }
    }
    (status, m, f)
}

pub struct StorageHealth;

impl DiagnosticTest for StorageHealth {
    fn descriptor(&self) -> TestDescriptor {
        desc(
            "BE-T-STO-002",
            "1.0.0",
            "Storage Health (SMART / NVMe log)",
            Subsystem::Storage,
            LoadClass::ReadOnly,
            Privilege::Elevated,
            5,
            "Reads SMART / NVMe health logs through an installed smartmontools `smartctl` (invoked as a separate program, JSON output). Read-only; no self-tests are started.",
            "SMART overall status, critical warnings, media errors, endurance, temperature",
            "Requires smartctl to be installed and usually administrator/root rights. Virtual disks do not provide SMART data.",
            json!({}),
        )
    }

    fn run(&self, ctx: &TestContext) -> TestResult {
        let d = self.descriptor();
        let started = Instant::now();
        let mut r = TestResult::new(&d, "All physical storage devices", ctx.provenance);
        let inv_devs: Vec<String> = ctx
            .platform
            .inventory()
            .storage
            .into_iter()
            .map(|s| s.name)
            .collect();
        let devs: Vec<String> = inv_devs
            .iter()
            .filter(|n| !n.starts_with("vd") && !n.starts_with("sr"))
            .cloned()
            .collect();
        if devs.is_empty() {
            r.notes.push(if inv_devs.is_empty() {
                "No storage devices found".into()
            } else {
                format!(
                    "Only virtual disks found ({}); SMART is not applicable",
                    inv_devs.join(", ")
                )
            });
            return finish(r, started, TestStatus::Unsupported);
        }
        let mut worst = TestStatus::Pass;
        let mut covered = Vec::new();
        for dev in &devs {
            let path = if cfg!(windows) {
                dev.clone()
            } else {
                format!("/dev/{dev}")
            };
            let Some(tool) = be_core::fsutil::trusted_tool("smartctl") else {
                r.notes.push("smartctl not found in system directories. Install smartmontools to enable this test.".into());
                return finish(r, started, TestStatus::Unsupported);
            };
            let out = Command::new(tool).args(["-j", "-H", "-A", &path]).output();
            let out = match out {
                Ok(o) => o,
                Err(e) => {
                    r.notes.push(format!(
                        "smartctl not available: {e}. Install smartmontools to enable this test."
                    ));
                    return finish(r, started, TestStatus::Unsupported);
                }
            };
            let v: Value = match serde_json::from_slice(&out.stdout) {
                Ok(v) => v,
                Err(_) => {
                    r.notes.push(format!(
                        "{dev}: smartctl produced no JSON (exit {})",
                        out.status
                    ));
                    worst = worst.max_by_severity(TestStatus::Inconclusive);
                    continue;
                }
            };
            // smartctl exit bit 1: device open failed (commonly permissions).
            let code = out.status.code().unwrap_or(-1);
            if code >= 0 && code & 0b10 != 0 {
                r.notes.push(format!(
                    "{dev}: smartctl could not open the device (requires administrator/root)"
                ));
                worst = worst.max_by_severity(TestStatus::Inconclusive);
                continue;
            }
            let (st, m, f) = evaluate_smartctl(dev, &v);
            r.measurements.extend(m);
            r.faults.extend(f);
            covered.push(dev.clone());
            worst = worst.max_by_severity(st);
        }
        r.coverage = format!(
            "SMART/NVMe health read for: {}",
            if covered.is_empty() {
                "none".into()
            } else {
                covered.join(", ")
            }
        );
        if covered.is_empty() && worst == TestStatus::Pass {
            worst = TestStatus::Inconclusive;
        }
        finish(r, started, worst)
    }
}

trait SeverityMax {
    fn max_by_severity(self, other: TestStatus) -> TestStatus;
}
impl SeverityMax for TestStatus {
    fn max_by_severity(self, other: TestStatus) -> TestStatus {
        if other.severity() > self.severity() {
            other
        } else {
            self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smartctl_nvme_fixture() {
        // FIXTURE: synthetic smartctl JSON.
        let v = json!({"smart_status":{"passed":true},
            "nvme_smart_health_information_log":{"critical_warning":0,"temperature":38,"available_spare":100,
              "percentage_used":2,"media_errors":0,"num_err_log_entries":5,"unsafe_shutdowns":3,"power_on_hours":120}});
        let (st, m, f) = evaluate_smartctl("nvme0n1", &v);
        assert_eq!(st, TestStatus::Pass);
        assert!(f.is_empty());
        assert!(
            m.iter()
                .any(|x| x.name.ends_with("temperature") && x.value == Some(38.0))
        );

        let bad = json!({"smart_status":{"passed":false},"nvme_smart_health_information_log":{"critical_warning":4,"media_errors":7}});
        let (st, _, f) = evaluate_smartctl("nvme0n1", &bad);
        assert_eq!(st, TestStatus::Fail);
        assert!(f.iter().any(|x| x.code == "BE-STO-002"));
        assert!(f.iter().any(|x| x.code == "BE-STO-004"));
    }

    #[test]
    fn smartctl_missing_status_is_inconclusive() {
        let (st, m, _) = evaluate_smartctl("sda", &json!({}));
        assert_eq!(st, TestStatus::Inconclusive);
        assert_eq!(m[0].value, None);
    }

    #[test]
    fn ata_reallocated_sectors_warn() {
        let v = json!({"smart_status":{"passed":true},"ata_smart_attributes":{"table":[{"id":5,"name":"Reallocated_Sector_Ct","raw":{"value":12}}]}});
        let (st, _, f) = evaluate_smartctl("sda", &v);
        assert_eq!(st, TestStatus::Warning);
        assert_eq!(f[0].code, "BE-STO-004");
    }
}
