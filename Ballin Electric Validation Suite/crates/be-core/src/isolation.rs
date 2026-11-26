//! Deterministic, explainable fault isolation.
//!
//! Correlates results across a run to separate **confirmed observations**
//! from **hypotheses** (with confidence and the evidence supporting each),
//! suggests **next safe tests**, and gives escalation instructions. It never
//! labels a hypothesis as a diagnosed failed component, and it never
//! overrides the release policy.

use crate::model::{Provenance, TestResult};
use crate::profile::{Profile, ProfileStep};
use crate::status::TestStatus;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const RULESET_VERSION: &str = "1.0.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

impl Confidence {
    pub fn label(self) -> &'static str {
        match self {
            Confidence::Low => "low",
            Confidence::Medium => "medium",
            Confidence::High => "high",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hypothesis {
    pub cause: String,
    pub confidence: Confidence,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub rule: String,
    pub title: String,
    pub observations: Vec<String>,
    pub hypotheses: Vec<Hypothesis>,
    /// Test IDs to run next (safe, ordered).
    pub next_tests: Vec<String>,
    pub escalation: String,
    /// Unit should be held pending review.
    pub hold: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Analysis {
    pub ruleset_version: String,
    pub findings: Vec<Finding>,
    /// Ordered, de-duplicated follow-up test IDs across all findings.
    pub follow_up: Vec<String>,
    pub caveat: String,
}

struct Ctx<'a> {
    results: &'a [(String, TestResult)],
}

impl Ctx<'_> {
    fn has_code(&self, code: &str) -> bool {
        self.results
            .iter()
            .any(|(_, r)| r.faults.iter().any(|f| f.code == code))
    }
    fn code_obs(&self, code: &str) -> Vec<String> {
        self.results
            .iter()
            .flat_map(|(k, r)| {
                r.faults
                    .iter()
                    .filter(move |f| f.code == code)
                    .map(move |f| format!("{k}: {} — {}", f.message, f.observation))
            })
            .take(6)
            .collect()
    }
    fn status(&self, test_id: &str) -> Option<TestStatus> {
        // Worst status of any step running this test.
        self.results
            .iter()
            .filter(|(_, r)| r.test_id == test_id)
            .map(|(_, r)| r.status)
            .max_by_key(|s| s.severity())
    }
    fn passed(&self, test_id: &str) -> bool {
        self.status(test_id) == Some(TestStatus::Pass)
    }
    fn failed(&self, test_id: &str) -> bool {
        self.status(test_id) == Some(TestStatus::Fail)
    }
    fn devices_with(&self, code: &str) -> BTreeSet<String> {
        self.results
            .iter()
            .flat_map(|(_, r)| {
                r.faults
                    .iter()
                    .filter(|f| f.code == code)
                    .filter_map(|f| f.device.clone())
            })
            .collect()
    }
}

fn h(cause: &str, c: Confidence, evidence: &[&str]) -> Hypothesis {
    Hypothesis {
        cause: cause.into(),
        confidence: c,
        evidence: evidence.iter().map(|s| s.to_string()).collect(),
    }
}

/// Analyse the results of one run.
pub fn analyze(results: &[(String, TestResult)]) -> Analysis {
    let c = Ctx { results };
    let mut f: Vec<Finding> = Vec::new();

    // --- CPU arithmetic mismatch
    if c.has_code("BE-CPU-001") {
        let mem_fail = c.has_code("BE-MEM-001") || c.has_code("BE-MEM-003");
        let mut hy = vec![];
        if mem_fail {
            hy.push(h(
                "System memory error corrupting CPU work",
                Confidence::High,
                &["memory pattern or ECC errors in the same run"],
            ));
            hy.push(h(
                "CPU core / cache instability",
                Confidence::Low,
                &["arithmetic mismatch"],
            ));
        } else {
            hy.push(h(
                "CPU core or cache instability",
                Confidence::Medium,
                &[
                    "verified arithmetic mismatch",
                    "memory tests did not fail in this run",
                ],
            ));
            hy.push(h(
                "Unstable firmware settings (overclock, undervolt, memory profile)",
                Confidence::Medium,
                &["mismatches are typical of marginal settings"],
            ));
            hy.push(h(
                "Memory controller or memory not covered by the OS test",
                Confidence::Low,
                &["OS memory tests cover only allocated buffers"],
            ));
        }
        if c.has_code("BE-THERM-001") || c.has_code("BE-THERM-002") {
            hy.push(h(
                "Thermal stress during the workload",
                Confidence::Medium,
                &["temperature warnings in the same run"],
            ));
        }
        f.push(Finding {
            rule: "ISO-CPU-01".into(),
            title: "CPU computation produced wrong answers".into(),
            observations: c.code_obs("BE-CPU-001"),
            hypotheses: hy,
            next_tests: vec!["BE-T-CPU-001".into(), "BE-T-MEM-001".into(), "BE-T-OS-001".into(), "BE-T-FW-001".into()],
            escalation: "Hold the unit. Restore approved firmware defaults, repeat, and escalate to engineering if mismatches persist on stock settings.".into(),
            hold: true,
        });
    }

    // --- System memory
    if c.has_code("BE-MEM-001") || c.has_code("BE-MEM-003") {
        let cpu_ok = c.passed("BE-T-CPU-001");
        let mut hy = vec![h(
            "DIMM defect or poor DIMM contact",
            if cpu_ok {
                Confidence::High
            } else {
                Confidence::Medium
            },
            &["memory pattern/ECC errors"],
        )];
        hy.push(h(
            "Memory settings beyond validated profile (XMP/EXPO, voltage)",
            Confidence::Medium,
            &["pattern errors under load"],
        ));
        hy.push(h(
            "CPU memory controller",
            Confidence::Low,
            &["cannot be separated from DIMMs without swap testing"],
        ));
        f.push(Finding {
            rule: "ISO-MEM-01".into(),
            title: "System memory returned incorrect data or reported ECC errors".into(),
            observations: [c.code_obs("BE-MEM-001"), c.code_obs("BE-MEM-003")].concat(),
            hypotheses: hy,
            next_tests: vec!["BE-T-MEM-001".into(), "BE-T-OS-001".into()],
            escalation: "Hold the unit. With power removed, reseat DIMMs; test DIMMs individually in a known-good slot; run a preboot memory test for full coverage.".into(),
            hold: true,
        });
    }

    // --- GPU memory / compute / rendering
    let vram_devs = c.devices_with("BE-GPU-014");
    let comp_devs = c.devices_with("BE-GPU-010");
    if !vram_devs.is_empty() || !comp_devs.is_empty() || c.has_code("BE-GPU-030") {
        let both: Vec<_> = vram_devs.intersection(&comp_devs).cloned().collect();
        let all: BTreeSet<_> = vram_devs.union(&comp_devs).cloned().collect();
        let mut hy = vec![];
        if all.len() > 1 {
            hy.push(h(
                "Shared cause across GPUs (power delivery, driver, system memory)",
                Confidence::Medium,
                &["errors on more than one GPU"],
            ));
        }
        if !both.is_empty() {
            hy.push(h(
                "GPU core or VRAM defect on the affected device",
                Confidence::Medium,
                &["compute and VRAM errors on the same device"],
            ));
        } else if !vram_devs.is_empty() {
            hy.push(h(
                "VRAM defect on the affected device",
                Confidence::Medium,
                &["VRAM pattern mismatch"],
            ));
        }
        hy.push(h(
            "Unstable power to the GPU (PSU rail, connector, riser)",
            Confidence::Low,
            &["errors under load can stem from power"],
        ));
        hy.push(h(
            "Driver defect",
            Confidence::Low,
            &["compare with the validated driver version"],
        ));
        if c.has_code("BE-GPU-021") || c.has_code("BE-THERM-001") {
            hy.push(h(
                "Overheating or throttling",
                Confidence::Medium,
                &["throttling/thermal warnings in the same run"],
            ));
        }
        f.push(Finding {
            rule: "ISO-GPU-01".into(),
            title: "GPU produced incorrect results".into(),
            observations: [c.code_obs("BE-GPU-014"), c.code_obs("BE-GPU-010"), c.code_obs("BE-GPU-030")].concat(),
            hypotheses: hy,
            next_tests: vec!["BE-T-GPU-002".into(), "BE-T-GPU-003".into(), "BE-T-GPU-001".into(), "BE-T-OS-001".into()],
            escalation: "Hold the unit. Repeat each GPU independently; swap the suspect GPU into a known-good slot/riser or test a known-good GPU in this slot; review ECC counters where supported.".into(),
            hold: true,
        });
    }

    // --- GPU lost / hang, with PCIe evidence
    if c.has_code("BE-GPU-011") {
        let pcie =
            c.has_code("BE-PCIE-003") || c.has_code("BE-PCIE-004") || c.has_code("BE-PCIE-002");
        let mut hy = vec![];
        if pcie {
            hy.push(h(
                "PCIe riser, cable or slot signal integrity",
                Confidence::High,
                &["device loss with PCIe errors/downshift in the same run"],
            ));
        } else {
            hy.push(h(
                "PCIe riser, cable or slot",
                Confidence::Medium,
                &["device disappeared under load"],
            ));
        }
        hy.push(h(
            "Insufficient or unstable GPU power (PSU, connectors)",
            Confidence::Medium,
            &["device loss under load"],
        ));
        hy.push(h(
            "Driver reset (TDR) or driver defect",
            Confidence::Low,
            &["see system log"],
        ));
        hy.push(h(
            "GPU hardware fault",
            Confidence::Low,
            &["requires isolation"],
        ));
        f.push(Finding {
            rule: "ISO-GPU-02".into(),
            title: "GPU device was lost or hung".into(),
            observations: [c.code_obs("BE-GPU-011"), c.code_obs("BE-PCIE-003"), c.code_obs("BE-PCIE-004")].concat(),
            hypotheses: hy,
            next_tests: vec!["BE-T-PCIE-001".into(), "BE-T-OS-001".into(), "BE-T-GPU-001".into(), "BE-T-GPU-002".into()],
            escalation: "Hold the unit. A technician must inspect riser, seating and PCIe power connectors with power removed, then retest each GPU in isolation.".into(),
            hold: true,
        });
    }

    // --- Individual tests pass, combined/simultaneous load fails
    let combined_fail = c.failed("BE-T-SYS-001")
        || results.iter().any(|(k, r)| {
            k.contains("simultaneous") && matches!(r.status, TestStatus::Fail | TestStatus::Aborted)
        });
    let isolated_ok = c.passed("BE-T-CPU-STRESS") || c.passed("BE-T-CPU-001");
    if combined_fail && isolated_ok {
        f.push(Finding {
            rule: "ISO-SYS-01".into(),
            title: "Individual tests pass but combined load fails".into(),
            observations: results
                .iter()
                .filter(|(k, r)| (r.test_id == "BE-T-SYS-001" || k.contains("simultaneous")) && matches!(r.status, TestStatus::Fail | TestStatus::Aborted))
                .map(|(k, r)| format!("{k}: {}", r.status.label()))
                .collect(),
            hypotheses: vec![
                h("Power delivery limit (PSU capacity, transient response, connectors)", Confidence::Medium, &["failure only when loads combine"]),
                h("Shared thermal limit (chassis airflow, heat recirculation)", Confidence::Medium, &["failure only when heat sources combine"]),
                h("Motherboard VRM or slot power", Confidence::Low, &["combined current draw"]),
            ],
            next_tests: vec!["BE-T-SNS-001".into(), "BE-T-OS-001".into()],
            escalation: "Hold the unit. Review temperatures and logs; perform qualified external power measurement (software telemetry cannot certify PSU behaviour).".into(),
            hold: true,
        });
    }

    // --- Thermal
    if c.has_code("BE-THERM-002") || c.has_code("BE-GPU-021") || c.has_code("BE-THERM-003") {
        f.push(Finding {
            rule: "ISO-THERM-01".into(),
            title: "Thermal limit, throttling or fan problem observed".into(),
            observations: [c.code_obs("BE-THERM-002"), c.code_obs("BE-GPU-021"), c.code_obs("BE-THERM-003"), c.code_obs("BE-THERM-001")].concat(),
            hypotheses: vec![
                h("Cooling obstruction or fan fault", Confidence::Medium, &["temperature/throttle/fan observations"]),
                h("Thermal interface (paste/pad) problem", Confidence::Medium, &["high temperature under load"]),
                h("Ambient temperature above the approved envelope", Confidence::Low, &["record ambient"]),
            ],
            next_tests: vec!["BE-T-SNS-001".into()],
            escalation: "Allow cooldown. Inspect airflow, fans and heatsink mounting with power removed before any retest.".into(),
            hold: c.has_code("BE-THERM-002"),
        });
    }

    // --- Storage
    if c.has_code("BE-STO-001") || c.has_code("BE-STO-002") || c.has_code("BE-STO-004") {
        let smart_bad = c.has_code("BE-STO-002") || c.has_code("BE-STO-004");
        let mut hy = vec![];
        if smart_bad {
            hy.push(h(
                "Storage device media or controller degradation",
                Confidence::High,
                &["device health log reports errors"],
            ));
        } else {
            hy.push(h(
                "Storage device or controller",
                Confidence::Medium,
                &["data integrity error"],
            ));
            hy.push(h(
                "System memory corrupting I/O buffers",
                Confidence::Low,
                &["run memory tests"],
            ));
        }
        hy.push(h(
            "Mounting, cooling or connector of the drive",
            Confidence::Low,
            &["intermittent errors"],
        ));
        f.push(Finding {
            rule: "ISO-STO-01".into(),
            title: "Storage integrity or health problem".into(),
            observations: [c.code_obs("BE-STO-001"), c.code_obs("BE-STO-002"), c.code_obs("BE-STO-004")].concat(),
            hypotheses: hy,
            next_tests: vec!["BE-T-STO-002".into(), "BE-T-STO-001".into(), "BE-T-MEM-001".into(), "BE-T-OS-001".into()],
            escalation: "Back up data. Review SMART/NVMe logs; inspect drive mounting and cooling; replace per service procedure if health is critical.".into(),
            hold: true,
        });
    }

    // --- PCIe link without device loss
    if (c.has_code("BE-PCIE-002") || c.has_code("BE-PCIE-003") || c.has_code("BE-PCIE-004"))
        && !c.has_code("BE-GPU-011")
    {
        f.push(Finding {
            rule: "ISO-PCIE-01".into(),
            title: "PCIe link errors or link below required width/speed".into(),
            observations: [
                c.code_obs("BE-PCIE-002"),
                c.code_obs("BE-PCIE-003"),
                c.code_obs("BE-PCIE-004"),
            ]
            .concat(),
            hypotheses: vec![
                h(
                    "Riser, cable or slot signal integrity",
                    Confidence::Medium,
                    &["AER/replay errors or reduced link"],
                ),
                h(
                    "Device not fully seated",
                    Confidence::Medium,
                    &["link width below expected"],
                ),
                h(
                    "Firmware link-speed setting",
                    Confidence::Low,
                    &["compare with approved configuration"],
                ),
            ],
            next_tests: vec!["BE-T-PCIE-001".into(), "BE-T-GPU-002".into()],
            escalation: "Reseat the device and riser with power removed; repeat under load.".into(),
            hold: c.has_code("BE-PCIE-002"),
        });
    }

    // --- Missing telemetry
    if c.has_code("BE-SNS-002") || c.has_code("BE-SNS-001") || c.has_code("BE-GPU-020") {
        f.push(Finding {
            rule: "ISO-SNS-01".into(),
            title: "Required telemetry is missing".into(),
            observations: [c.code_obs("BE-SNS-002"), c.code_obs("BE-SNS-001"), c.code_obs("BE-GPU-020")].concat(),
            hypotheses: vec![
                h("Sensor driver or vendor GPU driver not loaded", Confidence::Medium, &["no readings through supported interfaces"]),
                h("Virtual machine or platform without exposed sensors", Confidence::Medium, &["typical of VMs"]),
            ],
            next_tests: vec!["BE-T-SNS-001".into(), "BE-T-GPU-001".into()],
            escalation: "High-load testing stays blocked until required sensors report. Install supported drivers.".into(),
            hold: false,
        });
    }

    // --- Configuration mismatch
    if c.has_code("BE-CFG-001") {
        f.push(Finding {
            rule: "ISO-CFG-01".into(),
            title: "Hardware does not match the approved configuration".into(),
            observations: c.code_obs("BE-CFG-001"),
            hypotheses: vec![
                h("Wrong or missing part installed", Confidence::Medium, &["manifest comparison"]),
                h("Device not detected (seating, power, riser)", Confidence::Medium, &["fewer devices than expected"]),
                h("Manifest error", Confidence::Low, &["verify manifest under change control"]),
            ],
            next_tests: vec!["BE-T-INV-001".into(), "BE-T-PCIE-001".into()],
            escalation: "Hold the unit; verify the BOM and seating; correct the manifest only through approved change control.".into(),
            hold: true,
        });
    }

    let mut follow = Vec::new();
    for fi in &f {
        for t in &fi.next_tests {
            if !follow.contains(t) {
                follow.push(t.clone());
            }
        }
    }
    Analysis {
        ruleset_version: RULESET_VERSION.into(),
        findings: f,
        follow_up: follow,
        caveat: "Hypotheses are ranked possibilities based on correlated test evidence; they do not establish root cause. Confirm with the suggested tests and physical inspection.".into(),
    }
}

/// Build a follow-up diagnostic profile from an analysis. Never
/// release-qualifying. High-load tests are excluded (listed in the
/// description) because they need separate authorization.
pub fn follow_up_profile(
    a: &Analysis,
    is_high_load: &dyn Fn(&str) -> bool,
    run_id: Option<i64>,
) -> Option<Profile> {
    let mut excluded = Vec::new();
    let steps: Vec<ProfileStep> = a
        .follow_up
        .iter()
        .filter(|t| {
            let hl = is_high_load(t);
            if hl {
                excluded.push((*t).clone());
            }
            !hl
        })
        .map(|t| ProfileStep {
            key: None,
            test: t.clone(),
            required: true,
            params: serde_json::Value::Null,
            timeout_s: 1800,
            requires: vec![],
            waivable: false,
        })
        .collect();
    if steps.is_empty() {
        return None;
    }
    Some(Profile {
        schema: crate::profile::PROFILE_SCHEMA_VERSION,
        id: format!(
            "follow-up{}",
            run_id.map(|r| format!("-run{r}")).unwrap_or_default()
        ),
        version: RULESET_VERSION.into(),
        name: "Automatic follow-up diagnostics".into(),
        description: format!(
            "Generated by fault isolation ruleset {RULESET_VERSION}{}. Diagnostic only; never release-qualifying.{}",
            run_id.map(|r| format!(" from run {r}")).unwrap_or_default(),
            if excluded.is_empty() {
                String::new()
            } else {
                format!(
                    " High-load tests needing authorization: {}.",
                    excluded.join(", ")
                )
            }
        ),
        planned_minutes: None,
        allow_warnings: false,
        release_profile: false,
        steps,
        manual_checks: vec![],
    })
}

/// Results are evidence only when they came from live hardware.
pub fn live_only(results: &[(String, TestResult)]) -> bool {
    results
        .iter()
        .all(|(_, r)| r.provenance == Provenance::Live)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::FaultRecord;

    fn r(
        key: &str,
        id: &str,
        st: TestStatus,
        codes: &[(&str, Option<&str>)],
    ) -> (String, TestResult) {
        let mut t = TestResult::not_executed(id, "1", TestStatus::NotRun, "", Provenance::Live);
        t.status = st;
        for (c, d) in codes {
            let mut f = FaultRecord::new(c, "m", "o");
            f.device = d.map(String::from);
            t.faults.push(f);
        }
        (key.into(), t)
    }

    #[test]
    fn clean_run_has_no_findings() {
        let a = analyze(&[r("BE-T-CPU-001", "BE-T-CPU-001", TestStatus::Pass, &[])]);
        assert!(a.findings.is_empty());
        assert!(follow_up_profile(&a, &|_| false, None).is_none());
    }

    #[test]
    fn cpu_mismatch_with_memory_errors_points_to_memory() {
        let a = analyze(&[
            r(
                "BE-T-CPU-001",
                "BE-T-CPU-001",
                TestStatus::Fail,
                &[("BE-CPU-001", None)],
            ),
            r(
                "BE-T-MEM-001",
                "BE-T-MEM-001",
                TestStatus::Fail,
                &[("BE-MEM-001", None)],
            ),
        ]);
        let cpu = a.findings.iter().find(|f| f.rule == "ISO-CPU-01").unwrap();
        assert_eq!(cpu.hypotheses[0].confidence, Confidence::High);
        assert!(cpu.hypotheses[0].cause.contains("memory"));
        assert!(a.findings.iter().any(|f| f.rule == "ISO-MEM-01"));
        assert!(a.findings.iter().all(|f| f.hold));
    }

    #[test]
    fn device_loss_with_pcie_errors_points_to_riser() {
        let a = analyze(&[
            r(
                "g",
                "BE-T-GPU-004",
                TestStatus::Fail,
                &[("BE-GPU-011", Some("0000:01:00.0"))],
            ),
            r(
                "p",
                "BE-T-PCIE-001",
                TestStatus::Warning,
                &[("BE-PCIE-003", Some("0000:01:00.0"))],
            ),
        ]);
        let g = a.findings.iter().find(|f| f.rule == "ISO-GPU-02").unwrap();
        assert_eq!(g.hypotheses[0].confidence, Confidence::High);
        assert!(g.hypotheses[0].cause.contains("riser"));
        assert!(
            !a.findings.iter().any(|f| f.rule == "ISO-PCIE-01"),
            "PCIe folded into device-loss finding"
        );
    }

    #[test]
    fn errors_on_two_gpus_suggest_shared_cause() {
        let a = analyze(&[r(
            "v",
            "BE-T-GPU-003",
            TestStatus::Fail,
            &[("BE-GPU-014", Some("a")), ("BE-GPU-014", Some("b"))],
        )]);
        let g = a.findings.iter().find(|f| f.rule == "ISO-GPU-01").unwrap();
        assert!(g.hypotheses[0].cause.starts_with("Shared cause"));
    }

    #[test]
    fn combined_only_failure_points_to_power_and_thermal() {
        let a = analyze(&[
            r("BE-T-CPU-001", "BE-T-CPU-001", TestStatus::Pass, &[]),
            r("BE-T-SYS-001", "BE-T-SYS-001", TestStatus::Fail, &[]),
        ]);
        let s = a.findings.iter().find(|f| f.rule == "ISO-SYS-01").unwrap();
        assert!(s.hypotheses[0].cause.contains("Power"));
    }

    #[test]
    fn follow_up_profile_excludes_high_load_and_never_releases() {
        let a = analyze(&[r(
            "BE-T-STO-001",
            "BE-T-STO-001",
            TestStatus::Fail,
            &[("BE-STO-001", None)],
        )]);
        let p = follow_up_profile(&a, &|t| t == "BE-T-MEM-001", Some(7)).unwrap();
        assert!(!p.release_profile);
        assert!(p.steps.iter().all(|s| s.test != "BE-T-MEM-001"));
        assert!(p.description.contains("BE-T-MEM-001"));
        assert_eq!(p.id, "follow-up-run7");
        p.validate(&|_| true).unwrap();
    }
}
