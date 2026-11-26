//! Test descriptors and typed results.

use crate::status::TestStatus;
use crate::time::UtcMillis;
use serde::{Deserialize, Serialize};

/// Hardware subsystem a test targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Subsystem {
    Cpu,
    Memory,
    Gpu,
    Vram,
    Storage,
    Pcie,
    Motherboard,
    Power,
    Cooling,
    Display,
    Network,
    Usb,
    Firmware,
    Os,
    Sensors,
    System,
}

impl Subsystem {
    pub fn label(self) -> &'static str {
        match self {
            Subsystem::Cpu => "CPU",
            Subsystem::Memory => "System Memory",
            Subsystem::Gpu => "GPU",
            Subsystem::Vram => "VRAM",
            Subsystem::Storage => "Storage",
            Subsystem::Pcie => "PCIe",
            Subsystem::Motherboard => "Motherboard",
            Subsystem::Power => "Power",
            Subsystem::Cooling => "Cooling",
            Subsystem::Display => "Display",
            Subsystem::Network => "Network",
            Subsystem::Usb => "USB",
            Subsystem::Firmware => "Firmware",
            Subsystem::Os => "OS / Drivers",
            Subsystem::Sensors => "Sensors",
            Subsystem::System => "System",
        }
    }
}

/// Expected load the test places on the hardware. `High` requires explicit
/// operator authorization and an approved hardware safety profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoadClass {
    /// Reads information only.
    ReadOnly,
    /// Short, bounded computation or I/O (seconds, small buffers).
    Low,
    /// Sustained but bounded load (minutes, moderate buffers).
    Moderate,
    /// Stress / burn-in workload. Never started without authorization.
    High,
}

impl LoadClass {
    pub fn label(self) -> &'static str {
        match self {
            LoadClass::ReadOnly => "Read-only",
            LoadClass::Low => "Low",
            LoadClass::Moderate => "Moderate",
            LoadClass::High => "High (stress)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Privilege {
    Standard,
    /// Some readings need elevated rights; the test degrades gracefully.
    ElevatedOptional,
    Elevated,
}

/// Static declaration of a diagnostic test module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestDescriptor {
    pub id: String,
    pub version: String,
    pub name: String,
    pub subsystem: Subsystem,
    pub description: String,
    pub load: LoadClass,
    pub privilege: Privilege,
    pub destructive: bool,
    /// Typical duration with default parameters, in seconds (planning value).
    pub typical_duration_s: u64,
    pub cancel_behavior: String,
    pub evidence: String,
    pub limitations: String,
    pub manual_steps: Vec<String>,
    /// Default parameters (JSON object), overridable by profiles.
    pub default_params: serde_json::Value,
}

/// Where a result came from. Fixture results are never valid for release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    /// Executed against the live system.
    Live,
    /// Produced by an explicitly enabled development/test fixture.
    Fixture,
}

/// Quality of a measured value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataQuality {
    Measured,
    Derived,
    Unavailable,
    Stale,
}

/// A measured value. `value == None` means unknown; never substitute zero.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Measurement {
    pub name: String,
    pub value: Option<f64>,
    pub text: Option<String>,
    pub unit: String,
    pub quality: DataQuality,
}

impl Measurement {
    pub fn num(name: impl Into<String>, value: f64, unit: impl Into<String>) -> Self {
        Measurement {
            name: name.into(),
            value: Some(value),
            text: None,
            unit: unit.into(),
            quality: DataQuality::Measured,
        }
    }
    pub fn derived(name: impl Into<String>, value: f64, unit: impl Into<String>) -> Self {
        Measurement {
            quality: DataQuality::Derived,
            ..Measurement::num(name, value, unit)
        }
    }
    pub fn text(name: impl Into<String>, text: impl Into<String>) -> Self {
        Measurement {
            name: name.into(),
            value: None,
            text: Some(text.into()),
            unit: String::new(),
            quality: DataQuality::Measured,
        }
    }
    pub fn unavailable(name: impl Into<String>, unit: impl Into<String>) -> Self {
        Measurement {
            name: name.into(),
            value: None,
            text: None,
            unit: unit.into(),
            quality: DataQuality::Unavailable,
        }
    }
    pub fn display_value(&self) -> String {
        match (&self.value, &self.text, self.quality) {
            (_, _, DataQuality::Unavailable) => "unavailable".into(),
            (Some(v), _, _) => {
                if v.fract() == 0.0 && v.abs() < 1e15 {
                    format!("{v:.0} {}", self.unit).trim().to_string()
                } else {
                    format!("{v:.3} {}", self.unit).trim().to_string()
                }
            }
            (None, Some(t), _) => t.clone(),
            (None, None, _) => "unknown".into(),
        }
    }
}

/// A fault observed during a test, referencing the fault catalog.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FaultRecord {
    pub code: String,
    pub message: String,
    /// Confirmed observation (what was measured). Root cause is never asserted here.
    pub observation: String,
    pub device: Option<String>,
}

impl FaultRecord {
    pub fn new(code: &str, message: impl Into<String>, observation: impl Into<String>) -> Self {
        FaultRecord {
            code: code.to_string(),
            message: message.into(),
            observation: observation.into(),
            device: None,
        }
    }
    pub fn on(mut self, device: impl Into<String>) -> Self {
        self.device = Some(device.into());
        self
    }
}

/// Result of a single test execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TestResult {
    pub test_id: String,
    pub test_version: String,
    pub engine_version: String,
    pub device: String,
    pub status: TestStatus,
    pub started_utc: UtcMillis,
    /// Monotonic duration of the execution in milliseconds.
    pub duration_ms: u64,
    /// Human-readable description of what was actually covered.
    pub coverage: String,
    pub measurements: Vec<Measurement>,
    pub faults: Vec<FaultRecord>,
    pub notes: Vec<String>,
    pub provenance: Provenance,
}

impl TestResult {
    pub fn new(desc: &TestDescriptor, device: impl Into<String>, provenance: Provenance) -> Self {
        TestResult {
            test_id: desc.id.clone(),
            test_version: desc.version.clone(),
            engine_version: crate::ENGINE_VERSION.to_string(),
            device: device.into(),
            status: TestStatus::Running,
            started_utc: UtcMillis::now(),
            duration_ms: 0,
            coverage: String::new(),
            measurements: Vec::new(),
            faults: Vec::new(),
            notes: Vec::new(),
            provenance,
        }
    }

    /// Construct a terminal result for a test that did not execute.
    pub fn not_executed(
        test_id: &str,
        test_version: &str,
        status: TestStatus,
        reason: impl Into<String>,
        provenance: Provenance,
    ) -> Self {
        debug_assert!(status != TestStatus::Pass);
        TestResult {
            test_id: test_id.to_string(),
            test_version: test_version.to_string(),
            engine_version: crate::ENGINE_VERSION.to_string(),
            device: String::new(),
            status,
            started_utc: UtcMillis::now(),
            duration_ms: 0,
            coverage: "none".into(),
            measurements: Vec::new(),
            faults: Vec::new(),
            notes: vec![reason.into()],
            provenance,
        }
    }

    pub fn first_fault_code(&self) -> Option<&str> {
        self.faults.first().map(|f| f.code.as_str())
    }
}

/// Severity of an event log entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EventLevel {
    Debug,
    Info,
    Pass,
    Warn,
    Error,
}

impl EventLevel {
    pub fn label(self) -> &'static str {
        match self {
            EventLevel::Debug => "DEBUG",
            EventLevel::Info => "INFO",
            EventLevel::Pass => "PASS",
            EventLevel::Warn => "WARN",
            EventLevel::Error => "ERROR",
        }
    }
    pub fn parse(s: &str) -> Option<EventLevel> {
        match s.trim().to_ascii_uppercase().as_str() {
            "DEBUG" => Some(EventLevel::Debug),
            "INFO" => Some(EventLevel::Info),
            "PASS" => Some(EventLevel::Pass),
            "WARN" | "WARNING" => Some(EventLevel::Warn),
            "ERROR" => Some(EventLevel::Error),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventRecord {
    pub ts: UtcMillis,
    pub level: EventLevel,
    pub code: Option<String>,
    pub message: String,
}

impl EventRecord {
    pub fn new(level: EventLevel, message: impl Into<String>) -> Self {
        EventRecord {
            ts: UtcMillis::now(),
            level,
            code: None,
            message: message.into(),
        }
    }
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }
    /// Support bundles retain diagnostic metadata and omit free-form event text.
    pub fn support_metadata(&self) -> serde_json::Value {
        serde_json::json!({"ts": self.ts, "level": self.level, "code": self.code})
    }
    pub fn line(&self) -> String {
        match &self.code {
            Some(c) if !self.message.starts_with(c.as_str()) => {
                format!(
                    "{} {:<5} {}: {}",
                    self.ts.hms(),
                    self.level.label(),
                    c,
                    self.message
                )
            }
            _ => format!(
                "{} {:<5} {}",
                self.ts.hms(),
                self.level.label(),
                self.message
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_is_not_zero() {
        let m = Measurement::unavailable("CPU Package", "°C");
        assert_eq!(m.value, None);
        assert_eq!(m.display_value(), "unavailable");
    }

    #[test]
    fn result_json_round_trip() {
        let r = TestResult::not_executed(
            "BE-T-X",
            "1.0",
            TestStatus::Unsupported,
            "no backend",
            Provenance::Live,
        );
        let j = serde_json::to_string(&r).unwrap();
        assert!(j.contains("\"UNSUPPORTED\""));
        let back: TestResult = serde_json::from_str(&j).unwrap();
        assert_eq!(back, r);
    }
}
