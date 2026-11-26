//! Test status semantics. Every status is distinct; nothing other than an
//! executed, verified test may ever become `Pass`.

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TestStatus {
    Pass,
    Fail,
    Warning,
    NotRun,
    Unsupported,
    Skipped,
    Aborted,
    Inconclusive,
    Running,
    Pending,
}

impl TestStatus {
    pub const ALL: [TestStatus; 10] = [
        TestStatus::Pass,
        TestStatus::Fail,
        TestStatus::Warning,
        TestStatus::NotRun,
        TestStatus::Unsupported,
        TestStatus::Skipped,
        TestStatus::Aborted,
        TestStatus::Inconclusive,
        TestStatus::Running,
        TestStatus::Pending,
    ];

    /// Human-readable label used in the UI, logs and reports.
    pub fn label(self) -> &'static str {
        match self {
            TestStatus::Pass => "PASS",
            TestStatus::Fail => "FAIL",
            TestStatus::Warning => "WARNING",
            TestStatus::NotRun => "NOT RUN",
            TestStatus::Unsupported => "UNSUPPORTED",
            TestStatus::Skipped => "SKIPPED",
            TestStatus::Aborted => "ABORTED",
            TestStatus::Inconclusive => "INCONCLUSIVE",
            TestStatus::Running => "RUNNING",
            TestStatus::Pending => "PENDING",
        }
    }

    pub fn from_label(s: &str) -> Option<TestStatus> {
        let norm = s.trim().to_ascii_uppercase().replace('_', " ");
        TestStatus::ALL.into_iter().find(|st| st.label() == norm)
    }

    /// True when the test finished executing (successfully or not).
    pub fn is_terminal(self) -> bool {
        !matches!(self, TestStatus::Running | TestStatus::Pending)
    }

    /// A required test only satisfies release criteria when it passed.
    /// Warnings are accepted only if the profile explicitly allows them.
    pub fn satisfies_required(self, warnings_allowed: bool) -> bool {
        match self {
            TestStatus::Pass => true,
            TestStatus::Warning => warnings_allowed,
            _ => false,
        }
    }

    /// Severity ordering used to aggregate results: higher is worse.
    pub fn severity(self) -> u8 {
        match self {
            TestStatus::Pass => 0,
            TestStatus::Skipped => 1,
            TestStatus::Warning => 2,
            TestStatus::Pending => 3,
            TestStatus::Running => 3,
            TestStatus::NotRun => 4,
            TestStatus::Unsupported => 5,
            TestStatus::Inconclusive => 6,
            TestStatus::Aborted => 7,
            TestStatus::Fail => 8,
        }
    }
}

impl fmt::Display for TestStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_round_trip() {
        for st in TestStatus::ALL {
            assert_eq!(TestStatus::from_label(st.label()), Some(st));
        }
        assert_eq!(TestStatus::from_label("not_run"), Some(TestStatus::NotRun));
    }

    #[test]
    fn only_pass_satisfies_required_by_default() {
        for st in TestStatus::ALL {
            assert_eq!(st.satisfies_required(false), st == TestStatus::Pass, "{st}");
        }
        assert!(TestStatus::Warning.satisfies_required(true));
        assert!(!TestStatus::Unsupported.satisfies_required(true));
        assert!(!TestStatus::Aborted.satisfies_required(true));
        assert!(!TestStatus::Skipped.satisfies_required(true));
    }

    #[test]
    fn fail_is_most_severe() {
        let worst = TestStatus::ALL
            .into_iter()
            .max_by_key(|s| s.severity())
            .unwrap();
        assert_eq!(worst, TestStatus::Fail);
    }
}
