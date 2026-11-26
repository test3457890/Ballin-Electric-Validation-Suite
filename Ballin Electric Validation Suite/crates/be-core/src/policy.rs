//! Release (shipment disposition) policy.
//!
//! Deterministic and conservative: any required test that did not pass
//! (failed, aborted, unsupported, skipped, not run, missing, inconclusive,
//! or fixture-generated) blocks release. Release itself always requires a
//! separate, deliberate, authorized approval action.

use crate::codes::{self, Severity};
use crate::model::{Provenance, TestResult};
use crate::profile::Profile;
use crate::status::TestStatus;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseState {
    AwaitingAssembly,
    AwaitingTest,
    Testing,
    Hold,
    Rework,
    AwaitingReview,
    Released,
}

impl ReleaseState {
    pub fn label(self) -> &'static str {
        match self {
            ReleaseState::AwaitingAssembly => "Awaiting Assembly",
            ReleaseState::AwaitingTest => "Awaiting Test",
            ReleaseState::Testing => "Testing",
            ReleaseState::Hold => "HOLD",
            ReleaseState::Rework => "Rework",
            ReleaseState::AwaitingReview => "Awaiting Review",
            ReleaseState::Released => "Released",
        }
    }
    pub fn parse(s: &str) -> Option<ReleaseState> {
        let all = [
            ReleaseState::AwaitingAssembly,
            ReleaseState::AwaitingTest,
            ReleaseState::Testing,
            ReleaseState::Hold,
            ReleaseState::Rework,
            ReleaseState::AwaitingReview,
            ReleaseState::Released,
        ];
        all.into_iter().find(|st| {
            st.label().eq_ignore_ascii_case(s)
                || serde_json::to_value(st)
                    .ok()
                    .and_then(|v| v.as_str().map(|x| x == s))
                    .unwrap_or(false)
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManualCheckStatus {
    Pass,
    Fail,
    NotDone,
    NotApplicable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManualCheckRecord {
    pub check_id: String,
    pub status: ManualCheckStatus,
    pub operator: String,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Waiver {
    pub step_key: String,
    pub reason: String,
    pub reviewer: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReleaseEvaluation {
    /// True when the unit may be presented for release review.
    pub eligible_for_review: bool,
    pub recommended_state: ReleaseState,
    pub blockers: Vec<String>,
    pub notes: Vec<String>,
}

/// Evaluate release eligibility.
///
/// `results` maps profile step key to the result for that step in the run
/// being evaluated (rework creates a new run; old results are never edited).
pub fn evaluate(
    profile: &Profile,
    results: &HashMap<String, TestResult>,
    manual: &[ManualCheckRecord],
    waivers: &[Waiver],
) -> ReleaseEvaluation {
    let mut blockers = Vec::new();
    let mut notes = Vec::new();
    let mut hold = false;
    if !profile.release_profile {
        blockers.push(format!(
            "profile `{}` is a diagnostic profile, not a release-qualifying acceptance profile",
            profile.id
        ));
    }

    for step in &profile.steps {
        let key = step.key();
        let res = results.get(key);
        let critical_fault = res
            .map(|r| {
                r.faults.iter().any(|f| {
                    codes::lookup(&f.code)
                        .map(|c| c.severity == Severity::Critical)
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);

        if let Some(r) = res {
            if matches!(r.status, TestStatus::Fail | TestStatus::Aborted) || critical_fault {
                hold = true;
            }
            if r.provenance == Provenance::Fixture {
                blockers.push(format!(
                    "{key}: result produced by a development fixture, not live hardware"
                ));
                continue;
            }
        }

        let satisfied = res
            .map(|r| r.status.satisfies_required(profile.allow_warnings))
            .unwrap_or(false);
        if satisfied {
            continue;
        }
        let status_text = res.map(|r| r.status.label()).unwrap_or("MISSING");
        if !step.required {
            if res.map(|r| r.status == TestStatus::Fail).unwrap_or(false) {
                blockers.push(format!(
                    "{key}: optional test FAILED; failures are never ignored"
                ));
            } else {
                notes.push(format!("{key}: optional test {status_text}"));
            }
            continue;
        }

        let waiver = waivers.iter().find(|w| w.step_key == key);
        let waivable_status = res
            .map(|r| {
                matches!(
                    r.status,
                    TestStatus::Unsupported | TestStatus::NotRun | TestStatus::Skipped
                )
            })
            .unwrap_or(false);
        match waiver {
            Some(w)
                if step.waivable
                    && waivable_status
                    && !critical_fault
                    && !w.reviewer.trim().is_empty()
                    && !w.reason.trim().is_empty() =>
            {
                notes.push(format!(
                    "{key}: {status_text} waived by {} ({})",
                    w.reviewer, w.reason
                ));
            }
            Some(_) => {
                blockers.push(format!(
                    "{key}: required test {status_text}; waiver not permitted for this step/status"
                ));
            }
            None => blockers.push(format!("{key}: required test {status_text}")),
        }
    }

    for def in &profile.manual_checks {
        let rec = manual.iter().rev().find(|m| m.check_id == def.id);
        match rec.map(|r| r.status) {
            Some(ManualCheckStatus::Pass) => {}
            Some(ManualCheckStatus::NotApplicable) if !def.required => {}
            Some(ManualCheckStatus::Fail) => {
                hold = true;
                blockers.push(format!("manual check `{}` FAILED", def.label));
            }
            _ if def.required => {
                blockers.push(format!("manual check `{}` not signed off", def.label))
            }
            _ => notes.push(format!(
                "optional manual check `{}` not completed",
                def.label
            )),
        }
    }

    let recommended_state = if hold {
        ReleaseState::Hold
    } else if blockers.is_empty() {
        ReleaseState::AwaitingReview
    } else {
        ReleaseState::AwaitingTest
    };
    ReleaseEvaluation {
        eligible_for_review: blockers.is_empty() && !hold,
        recommended_state,
        blockers,
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{FaultRecord, TestResult};
    use crate::profile::Profile;

    fn profile() -> Profile {
        Profile::from_json(
            r#"{"schema":1,"id":"p","version":"1","name":"P","release_profile":true,
            "steps":[{"test":"A"},{"test":"B","waivable":true},{"test":"C","required":false}],
            "manual_checks":[{"id":"fin","label":"Finish"},{"id":"pkg","label":"Packaging","required":false}]}"#,
        )
        .unwrap()
    }

    fn res(id: &str, st: TestStatus) -> (String, TestResult) {
        let mut r = TestResult::not_executed(id, "1", TestStatus::NotRun, "", Provenance::Live);
        r.status = st;
        (id.to_string(), r)
    }

    fn signed() -> Vec<ManualCheckRecord> {
        vec![ManualCheckRecord {
            check_id: "fin".into(),
            status: ManualCheckStatus::Pass,
            operator: "op".into(),
            note: String::new(),
        }]
    }

    #[test]
    fn all_pass_is_eligible_for_review_not_released() {
        let r: HashMap<_, _> = [res("A", TestStatus::Pass), res("B", TestStatus::Pass)].into();
        let e = evaluate(&profile(), &r, &signed(), &[]);
        assert!(e.eligible_for_review, "{:?}", e.blockers);
        assert_eq!(e.recommended_state, ReleaseState::AwaitingReview);
    }

    #[test]
    fn every_non_pass_required_status_blocks() {
        for st in TestStatus::ALL {
            if st == TestStatus::Pass {
                continue;
            }
            let r: HashMap<_, _> = [res("A", st), res("B", TestStatus::Pass)].into();
            let e = evaluate(&profile(), &r, &signed(), &[]);
            assert!(!e.eligible_for_review, "{st} must block");
        }
    }

    #[test]
    fn missing_result_and_manual_check_block() {
        let r: HashMap<_, _> = [res("A", TestStatus::Pass)].into();
        let e = evaluate(&profile(), &r, &[], &[]);
        assert!(!e.eligible_for_review);
        assert_eq!(e.blockers.len(), 2);
    }

    #[test]
    fn failure_causes_hold_even_if_optional() {
        let r: HashMap<_, _> = [
            res("A", TestStatus::Pass),
            res("B", TestStatus::Pass),
            res("C", TestStatus::Fail),
        ]
        .into();
        let e = evaluate(&profile(), &r, &signed(), &[]);
        assert!(!e.eligible_for_review);
        let r: HashMap<_, _> = [res("A", TestStatus::Fail), res("B", TestStatus::Pass)].into();
        assert_eq!(
            evaluate(&profile(), &r, &signed(), &[]).recommended_state,
            ReleaseState::Hold
        );
    }

    #[test]
    fn waivers_are_restricted() {
        let w = vec![Waiver {
            step_key: "B".into(),
            reason: "no fixture at station".into(),
            reviewer: "QA Lead".into(),
        }];
        let r: HashMap<_, _> = [
            res("A", TestStatus::Pass),
            res("B", TestStatus::Unsupported),
        ]
        .into();
        assert!(evaluate(&profile(), &r, &signed(), &w).eligible_for_review);
        // Failure cannot be waived.
        let r: HashMap<_, _> = [res("A", TestStatus::Pass), res("B", TestStatus::Fail)].into();
        assert!(!evaluate(&profile(), &r, &signed(), &w).eligible_for_review);
        // Non-waivable step.
        let wa = vec![Waiver {
            step_key: "A".into(),
            ..w[0].clone()
        }];
        let r: HashMap<_, _> = [
            res("A", TestStatus::Unsupported),
            res("B", TestStatus::Pass),
        ]
        .into();
        assert!(!evaluate(&profile(), &r, &signed(), &wa).eligible_for_review);
        // Waiver without reviewer identity.
        let wn = vec![Waiver {
            reviewer: " ".into(),
            ..w[0].clone()
        }];
        let r: HashMap<_, _> = [
            res("A", TestStatus::Pass),
            res("B", TestStatus::Unsupported),
        ]
        .into();
        assert!(!evaluate(&profile(), &r, &signed(), &wn).eligible_for_review);
    }

    #[test]
    fn critical_fault_and_fixtures_block() {
        let (_, mut a) = res("A", TestStatus::Warning);
        a.faults.push(FaultRecord::new("BE-THERM-002", "x", "y"));
        let mut p = profile();
        p.allow_warnings = true;
        let r: HashMap<_, _> = [("A".to_string(), a), res("B", TestStatus::Pass)].into();
        let e = evaluate(&p, &r, &signed(), &[]);
        assert_eq!(e.recommended_state, ReleaseState::Hold);

        let (_, mut f) = res("A", TestStatus::Pass);
        f.provenance = Provenance::Fixture;
        let r: HashMap<_, _> = [("A".to_string(), f), res("B", TestStatus::Pass)].into();
        assert!(!evaluate(&profile(), &r, &signed(), &[]).eligible_for_review);
    }

    #[test]
    fn diagnostic_profiles_never_release() {
        let mut p = profile();
        p.release_profile = false;
        let r: HashMap<_, _> = [res("A", TestStatus::Pass), res("B", TestStatus::Pass)].into();
        let e = evaluate(&p, &r, &signed(), &[]);
        assert!(!e.eligible_for_review);
        assert_eq!(e.recommended_state, ReleaseState::AwaitingTest);
    }

    #[test]
    fn failed_manual_check_holds() {
        let m = vec![ManualCheckRecord {
            check_id: "fin".into(),
            status: ManualCheckStatus::Fail,
            operator: "op".into(),
            note: "scratch".into(),
        }];
        let r: HashMap<_, _> = [res("A", TestStatus::Pass), res("B", TestStatus::Pass)].into();
        assert_eq!(
            evaluate(&profile(), &r, &m, &[]).recommended_state,
            ReleaseState::Hold
        );
    }
}
