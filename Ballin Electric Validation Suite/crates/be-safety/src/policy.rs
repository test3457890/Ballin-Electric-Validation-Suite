//! Authorization and approved hardware safety profiles.
//!
//! No universal temperature or power limits are assumed. High-load tests
//! require (1) explicit operator authorization and (2) a hardware safety
//! profile that has been marked approved by a named engineer. The repository
//! ships only an EXAMPLE profile that is deliberately NOT approved.

use be_core::inventory::{SensorKind, SensorReading};
use be_core::model::{LoadClass, TestDescriptor};
use serde::{Deserialize, Serialize};

pub const SAFETY_PROFILE_SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SensorRule {
    /// Case-insensitive substring matched against sensor id, chip or label.
    pub matches: String,
    pub kind: SensorKind,
    /// Warn when value >= warn (or <= warn for `below` rules).
    pub warn: Option<f64>,
    /// Abort the workload when value >= abort.
    pub abort: Option<f64>,
    /// Abort when value <= abort_below (e.g. fan RPM).
    #[serde(default)]
    pub abort_below: Option<f64>,
    /// Required sensors must exist before start and stay fresh during load.
    #[serde(default)]
    pub required: bool,
}

impl SensorRule {
    pub fn applies(&self, s: &SensorReading) -> bool {
        let m = self.matches.to_ascii_lowercase();
        s.kind == self.kind
            && (s.id.to_ascii_lowercase().contains(&m)
                || s.chip.to_ascii_lowercase().contains(&m)
                || s.label.to_ascii_lowercase().contains(&m))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HardwareSafetyProfile {
    pub schema: u32,
    pub id: String,
    pub version: String,
    pub description: String,
    /// Must be true, with a named approver, before any high-load use.
    pub approved: bool,
    #[serde(default)]
    pub approved_by: Option<String>,
    pub rules: Vec<SensorRule>,
    #[serde(default = "default_sample_ms")]
    pub sample_interval_ms: u64,
    /// Maximum consecutive failed samples of a required sensor before abort.
    #[serde(default = "default_max_missed")]
    pub max_missed_samples: u32,
    /// Cooldown observation after the workload ends, in seconds.
    #[serde(default)]
    pub cooldown_s: u64,
}

fn default_sample_ms() -> u64 {
    1000
}
fn default_max_missed() -> u32 {
    3
}

impl HardwareSafetyProfile {
    pub fn from_json(s: &str) -> Result<Self, String> {
        let p: HardwareSafetyProfile = serde_json::from_str(s).map_err(|e| e.to_string())?;
        if p.schema != SAFETY_PROFILE_SCHEMA {
            return Err(format!("unsupported safety profile schema {}", p.schema));
        }
        if p.rules.is_empty() {
            return Err("safety profile has no sensor rules".into());
        }
        for r in &p.rules {
            if r.abort.is_none() && r.abort_below.is_none() {
                return Err(format!("rule `{}` has no abort limit", r.matches));
            }
            if let (Some(w), Some(a)) = (r.warn, r.abort)
                && w > a
            {
                return Err(format!("rule `{}`: warn {w} above abort {a}", r.matches));
            }
        }
        if p.sample_interval_ms == 0 || p.sample_interval_ms > 10_000 {
            return Err("sample_interval_ms must be 1..=10000".into());
        }
        Ok(p)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Authorization {
    pub operator: String,
    /// Operator explicitly authorized high-load workloads for this run.
    pub high_load: bool,
    pub safety_profile: Option<HardwareSafetyProfile>,
}

/// Decide whether a test may start. Returns a human-readable refusal.
pub fn preflight(
    desc: &TestDescriptor,
    auth: &Authorization,
    sensors: &[SensorReading],
) -> Result<(), String> {
    if desc.destructive {
        return Err("Destructive tests are disabled in this release".into());
    }
    if desc.load < LoadClass::High {
        return Ok(());
    }
    if !auth.high_load {
        return Err(format!(
            "{} is a high-load test and requires explicit operator authorization",
            desc.name
        ));
    }
    let Some(p) = &auth.safety_profile else {
        return Err("No hardware safety profile supplied; high-load tests require an approved profile for this hardware".into());
    };
    if !p.approved
        || p.approved_by
            .as_deref()
            .map(str::trim)
            .unwrap_or("")
            .is_empty()
    {
        return Err(format!(
            "Safety profile `{}` is not approved by a named engineer",
            p.id
        ));
    }
    for rule in p.rules.iter().filter(|r| r.required) {
        let matching: Vec<_> = sensors.iter().filter(|s| rule.applies(s)).collect();
        if matching.is_empty() {
            return Err(format!(
                "Required sensor `{}` ({}) not found",
                rule.matches,
                rule.kind.label()
            ));
        }
        if matching.iter().all(|s| s.value.is_none()) {
            return Err(format!(
                "Required sensor `{}` has no current reading",
                rule.matches
            ));
        }
        for s in matching {
            if let (Some(v), Some(w)) = (s.value, rule.warn)
                && rule.abort_below.is_none()
                && v >= w
            {
                return Err(format!(
                    "{} {} is {v:.1} {} — already at/above warning limit {w}; allow cooldown",
                    s.chip, s.label, s.unit
                ));
            }
        }
    }
    if !p
        .rules
        .iter()
        .any(|r| r.required && r.kind == SensorKind::Temperature)
    {
        return Err("Safety profile must require at least one temperature sensor".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use be_core::time::UtcMillis;
    use serde_json::json;

    fn high() -> TestDescriptor {
        serde_json::from_value(json!({"id":"H","version":"1","name":"High","subsystem":"cpu","description":"","load":"high",
            "privilege":"standard","destructive":false,"typical_duration_s":1,"cancel_behavior":"","evidence":"","limitations":"",
            "manual_steps":[],"default_params":{}}))
        .unwrap()
    }

    fn profile(approved: bool) -> HardwareSafetyProfile {
        HardwareSafetyProfile::from_json(
            &json!({"schema":1,"id":"p","version":"1","description":"","approved":approved,"approved_by": if approved {"Eng"} else {""},
            "rules":[{"matches":"tctl","kind":"temperature","warn":80.0,"abort":90.0,"required":true}]})
            .to_string(),
        )
        .unwrap()
    }

    fn temp(v: Option<f64>) -> SensorReading {
        SensorReading {
            id: "hwmon:k10temp:hwmon0:temp1".into(),
            chip: "k10temp".into(),
            label: "Tctl".into(),
            kind: SensorKind::Temperature,
            value: v,
            unit: "°C".into(),
            crit: None,
            max: None,
            ts: UtcMillis(0),
            source: "t".into(),
        }
    }

    #[test]
    fn high_load_requires_everything() {
        let d = high();
        assert!(preflight(&d, &Authorization::default(), &[]).is_err());
        let mut a = Authorization {
            operator: "op".into(),
            high_load: true,
            safety_profile: None,
        };
        assert!(
            preflight(&d, &a, &[temp(Some(40.0))])
                .unwrap_err()
                .contains("No hardware safety profile")
        );
        a.safety_profile = Some(profile(false));
        assert!(
            preflight(&d, &a, &[temp(Some(40.0))])
                .unwrap_err()
                .contains("not approved")
        );
        a.safety_profile = Some(profile(true));
        assert!(preflight(&d, &a, &[]).unwrap_err().contains("not found"));
        assert!(
            preflight(&d, &a, &[temp(None)])
                .unwrap_err()
                .contains("no current reading")
        );
        assert!(
            preflight(&d, &a, &[temp(Some(85.0))])
                .unwrap_err()
                .contains("warning limit")
        );
        assert!(preflight(&d, &a, &[temp(Some(40.0))]).is_ok());
    }

    #[test]
    fn low_load_needs_no_authorization() {
        let mut d = high();
        d.load = LoadClass::Low;
        assert!(preflight(&d, &Authorization::default(), &[]).is_ok());
        d.destructive = true;
        assert!(preflight(&d, &Authorization::default(), &[]).is_err());
    }

    #[test]
    fn profile_validation() {
        assert!(HardwareSafetyProfile::from_json(&json!({"schema":1,"id":"p","version":"1","description":"","approved":true,"rules":[]}).to_string()).is_err());
        assert!(
            HardwareSafetyProfile::from_json(
                &json!({"schema":1,"id":"p","version":"1","description":"","approved":true,
            "rules":[{"matches":"x","kind":"temperature","warn":95.0,"abort":90.0}]})
                .to_string()
            )
            .is_err()
        );
        assert!(
            HardwareSafetyProfile::from_json(
                &json!({"schema":1,"id":"p","version":"1","description":"","approved":true,
            "rules":[{"matches":"x","kind":"temperature","warn":80.0}]})
                .to_string()
            )
            .is_err()
        );
    }
}
