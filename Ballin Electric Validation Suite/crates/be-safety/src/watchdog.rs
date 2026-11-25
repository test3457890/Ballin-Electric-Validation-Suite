//! Sensor watchdog evaluated by the supervisor while a workload runs.

use crate::policy::HardwareSafetyProfile;
use be_core::inventory::SensorReading;
use be_core::model::{Measurement, TestResult};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum SafetyVerdict {
    Ok,
    Warn { code: String, message: String },
    Abort { code: String, message: String },
}

pub trait SafetyMonitor: Send {
    fn sample_interval_ms(&self) -> u64;
    fn check(&mut self) -> Vec<SafetyVerdict>;
    /// Append telemetry summary (min/max/avg) to the result.
    fn summarize(&self, result: &mut TestResult);
}

#[derive(Default, Debug, Clone)]
struct Stat {
    label: String,
    unit: String,
    min: f64,
    max: f64,
    sum: f64,
    n: u64,
}

/// Watchdog driven by a sensor sampling closure (the platform provider in
/// production; a scripted sequence in tests).
pub struct SensorWatchdog {
    profile: HardwareSafetyProfile,
    sampler: Box<dyn FnMut() -> Vec<SensorReading> + Send>,
    missed: BTreeMap<String, u32>,
    warned: std::collections::BTreeSet<String>,
    stats: BTreeMap<String, Stat>,
}

impl SensorWatchdog {
    pub fn new(
        profile: HardwareSafetyProfile,
        sampler: Box<dyn FnMut() -> Vec<SensorReading> + Send>,
    ) -> Self {
        SensorWatchdog {
            profile,
            sampler,
            missed: BTreeMap::new(),
            warned: Default::default(),
            stats: BTreeMap::new(),
        }
    }
}

impl SafetyMonitor for SensorWatchdog {
    fn sample_interval_ms(&self) -> u64 {
        self.profile.sample_interval_ms
    }

    fn check(&mut self) -> Vec<SafetyVerdict> {
        let sample = (self.sampler)();
        let mut out = Vec::new();
        for (ri, rule) in self.profile.rules.iter().enumerate() {
            let matching: Vec<&SensorReading> = sample.iter().filter(|s| rule.applies(s)).collect();
            let fresh: Vec<&&SensorReading> =
                matching.iter().filter(|s| s.value.is_some()).collect();
            let key = format!("rule{ri}:{}", rule.matches);
            if fresh.is_empty() {
                if rule.required {
                    let m = self.missed.entry(key.clone()).or_insert(0);
                    *m += 1;
                    if *m >= self.profile.max_missed_samples {
                        out.push(SafetyVerdict::Abort {
                            code: "BE-SNS-001".into(),
                            message: format!("Required sensor `{}` produced no reading for {} consecutive samples", rule.matches, m),
                        });
                    }
                }
                continue;
            }
            self.missed.insert(key, 0);
            for s in fresh {
                let v = s.value.unwrap();
                let st = self.stats.entry(s.id.clone()).or_insert_with(|| Stat {
                    label: format!("{} {}", s.chip, s.label),
                    unit: s.unit.clone(),
                    min: f64::INFINITY,
                    max: f64::NEG_INFINITY,
                    ..Default::default()
                });
                st.min = st.min.min(v);
                st.max = st.max.max(v);
                st.sum += v;
                st.n += 1;
                if let Some(a) = rule.abort
                    && v >= a
                {
                    out.push(SafetyVerdict::Abort {
                        code: "BE-THERM-002".into(),
                        message: format!(
                            "{} {} = {v:.1} {} reached abort limit {a}",
                            s.chip, s.label, s.unit
                        ),
                    });
                    continue;
                }
                if let Some(b) = rule.abort_below
                    && v <= b
                {
                    out.push(SafetyVerdict::Abort {
                        code: "BE-THERM-003".into(),
                        message: format!(
                            "{} {} = {v:.0} {} at/below limit {b}",
                            s.chip, s.label, s.unit
                        ),
                    });
                    continue;
                }
                if let Some(w) = rule.warn {
                    let warn = if rule.abort_below.is_some() {
                        v <= w
                    } else {
                        v >= w
                    };
                    if warn && self.warned.insert(s.id.clone()) {
                        out.push(SafetyVerdict::Warn {
                            code: "BE-THERM-001".into(),
                            message: format!(
                                "{} {} = {v:.1} {} reached warning limit {w}",
                                s.chip, s.label, s.unit
                            ),
                        });
                    }
                }
            }
        }
        if out.is_empty() {
            out.push(SafetyVerdict::Ok);
        }
        out
    }

    fn summarize(&self, result: &mut TestResult) {
        for st in self.stats.values().filter(|s| s.n > 0) {
            result.measurements.push(Measurement::num(
                format!("{} min", st.label),
                st.min,
                st.unit.clone(),
            ));
            result.measurements.push(Measurement::num(
                format!("{} max", st.label),
                st.max,
                st.unit.clone(),
            ));
            result.measurements.push(Measurement::derived(
                format!("{} avg", st.label),
                st.sum / st.n as f64,
                st.unit.clone(),
            ));
        }
    }
}
