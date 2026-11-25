//! Versioned test profiles (JSON).

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use thiserror::Error;

pub const PROFILE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub schema: u32,
    pub id: String,
    pub version: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Planning duration range in minutes (informational).
    #[serde(default)]
    pub planned_minutes: Option<[u32; 2]>,
    /// Whether WARNING results satisfy required steps.
    #[serde(default)]
    pub allow_warnings: bool,
    pub steps: Vec<ProfileStep>,
    #[serde(default)]
    pub manual_checks: Vec<ManualCheckDef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileStep {
    /// Unique key within the profile (defaults to the test id).
    #[serde(default)]
    pub key: Option<String>,
    pub test: String,
    #[serde(default = "yes")]
    pub required: bool,
    #[serde(default)]
    pub params: serde_json::Value,
    #[serde(default = "default_timeout")]
    pub timeout_s: u64,
    /// Keys of steps that must PASS before this step may start.
    #[serde(default)]
    pub requires: Vec<String>,
    /// Whether an authorized reviewer may waive a non-failing, non-executed result.
    #[serde(default)]
    pub waivable: bool,
}

fn yes() -> bool {
    true
}
fn default_timeout() -> u64 {
    600
}

impl ProfileStep {
    pub fn key(&self) -> &str {
        self.key.as_deref().unwrap_or(&self.test)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManualCheckDef {
    pub id: String,
    pub label: String,
    #[serde(default = "yes")]
    pub required: bool,
    /// True when the check depends on an external instrument or lab procedure.
    #[serde(default)]
    pub instrument: bool,
}

#[derive(Debug, Error, PartialEq)]
pub enum ProfileError {
    #[error("profile JSON is invalid: {0}")]
    Parse(String),
    #[error("unsupported profile schema {0} (expected {PROFILE_SCHEMA_VERSION})")]
    Schema(u32),
    #[error("profile field `{0}` must not be empty")]
    Empty(&'static str),
    #[error("duplicate step key `{0}`")]
    DuplicateStep(String),
    #[error("step `{step}` requires `{prereq}`, which is not an earlier step")]
    BadPrerequisite { step: String, prereq: String },
    #[error("step `{0}` references unknown test")]
    UnknownTest(String),
    #[error("step `{0}` has a zero timeout")]
    ZeroTimeout(String),
    #[error("duplicate manual check `{0}`")]
    DuplicateCheck(String),
    #[error("step `{0}` params must be a JSON object")]
    BadParams(String),
}

impl Profile {
    pub fn from_json(s: &str) -> Result<Profile, ProfileError> {
        let p: Profile = serde_json::from_str(s).map_err(|e| ProfileError::Parse(e.to_string()))?;
        Ok(p)
    }

    /// Structural validation. `known_test` returns whether a test id exists.
    pub fn validate(&self, known_test: &dyn Fn(&str) -> bool) -> Result<(), ProfileError> {
        if self.schema != PROFILE_SCHEMA_VERSION {
            return Err(ProfileError::Schema(self.schema));
        }
        if self.id.trim().is_empty() {
            return Err(ProfileError::Empty("id"));
        }
        if self.version.trim().is_empty() {
            return Err(ProfileError::Empty("version"));
        }
        if self.steps.is_empty() {
            return Err(ProfileError::Empty("steps"));
        }
        let mut seen = HashSet::new();
        for step in &self.steps {
            let key = step.key().to_string();
            if !known_test(&step.test) {
                return Err(ProfileError::UnknownTest(key));
            }
            if step.timeout_s == 0 {
                return Err(ProfileError::ZeroTimeout(key));
            }
            if !(step.params.is_null() || step.params.is_object()) {
                return Err(ProfileError::BadParams(key));
            }
            for r in &step.requires {
                if !seen.contains(r.as_str()) {
                    return Err(ProfileError::BadPrerequisite {
                        step: key.clone(),
                        prereq: r.clone(),
                    });
                }
            }
            if !seen.insert(step.key().to_string()) {
                return Err(ProfileError::DuplicateStep(key));
            }
        }
        let mut checks = HashSet::new();
        for c in &self.manual_checks {
            if !checks.insert(c.id.as_str()) {
                return Err(ProfileError::DuplicateCheck(c.id.clone()));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> &'static str {
        r#"{"schema":1,"id":"t","version":"1.0.0","name":"T",
            "steps":[{"test":"A"},{"test":"B","requires":["A"],"required":false}],
            "manual_checks":[{"id":"m1","label":"Chassis finish"}]}"#
    }

    #[test]
    fn parses_with_defaults() {
        let p = Profile::from_json(sample()).unwrap();
        assert!(p.steps[0].required);
        assert_eq!(p.steps[0].timeout_s, 600);
        assert!(!p.steps[1].required);
        assert!(p.manual_checks[0].required);
        p.validate(&|_| true).unwrap();
    }

    #[test]
    fn rejects_bad_structures() {
        let p = Profile::from_json(sample()).unwrap();
        assert_eq!(
            p.validate(&|t| t == "A"),
            Err(ProfileError::UnknownTest("B".into()))
        );

        let mut q = p.clone();
        q.steps[1].requires = vec!["Z".into()];
        assert!(matches!(
            q.validate(&|_| true),
            Err(ProfileError::BadPrerequisite { .. })
        ));

        let mut q = p.clone();
        q.steps[1].test = "A".into();
        q.steps[1].requires.clear();
        assert_eq!(
            q.validate(&|_| true),
            Err(ProfileError::DuplicateStep("A".into()))
        );

        let mut q = p.clone();
        q.schema = 9;
        assert_eq!(q.validate(&|_| true), Err(ProfileError::Schema(9)));

        let mut q = p.clone();
        q.steps[0].params = serde_json::json!([1]);
        assert_eq!(
            q.validate(&|_| true),
            Err(ProfileError::BadParams("A".into()))
        );

        assert!(matches!(
            Profile::from_json("{"),
            Err(ProfileError::Parse(_))
        ));
    }
}
