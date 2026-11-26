//! Versioned BE Validation fault-code catalog.
//!
//! Codes are original Ballin Electric codes (not vendor codes). Each entry
//! separates the confirmed observation from *possible* causes; a code never
//! by itself diagnoses a failed component.

use serde::Serialize;

pub const CATALOG_VERSION: &str = "1.0.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Error,
    Critical,
}

#[derive(Debug, Clone, Serialize)]
pub struct FaultCode {
    pub code: &'static str,
    pub severity: Severity,
    pub title: &'static str,
    pub meaning: &'static str,
    pub possible_causes: &'static [&'static str],
    pub next_steps: &'static [&'static str],
}

macro_rules! fc {
    ($code:expr, $sev:ident, $title:expr, $meaning:expr, [$($c:expr),*], [$($n:expr),*]) => {
        FaultCode {
            code: $code,
            severity: Severity::$sev,
            title: $title,
            meaning: $meaning,
            possible_causes: &[$($c),*],
            next_steps: &[$($n),*],
        }
    };
}

pub static CATALOG: &[FaultCode] = &[
    fc!(
        "BE-CPU-001",
        Error,
        "CPU arithmetic mismatch",
        "A verified integer or floating-point computation produced a result different from its known answer or reference.",
        [
            "CPU core instability",
            "System memory error",
            "Memory controller",
            "Unstable firmware/overclock settings",
            "Thermal or power instability"
        ],
        [
            "Repeat the verified workload on the same core",
            "Run system memory pattern tests",
            "Confirm firmware settings are at approved defaults",
            "Review hardware error logs (MCE/WHEA)"
        ]
    ),
    fc!(
        "BE-CPU-002",
        Error,
        "CPU worker inconsistency",
        "Parallel workers computing the same deterministic workload disagreed with each other.",
        ["Core-specific instability", "Cache error", "Memory error"],
        [
            "Re-run with per-core affinity",
            "Inspect machine-check logs",
            "Run memory tests"
        ]
    ),
    fc!(
        "BE-CPU-003",
        Warning,
        "Machine-check events reported",
        "The operating system recorded machine-check / hardware error events.",
        ["CPU, cache or memory errors", "Platform/firmware errors"],
        [
            "Review the referenced kernel/WHEA events",
            "Correlate timestamps with test activity"
        ]
    ),
    fc!(
        "BE-MEM-001",
        Error,
        "Memory pattern mismatch",
        "Data read back from an allocated memory buffer did not match the written pattern.",
        [
            "DIMM defect",
            "Memory controller",
            "Unstable memory timings/voltage",
            "Thermal issue"
        ],
        [
            "Repeat the pattern test",
            "Test DIMMs individually in a supported slot",
            "Run a preboot memory test for full coverage",
            "Reset memory settings to approved profile"
        ]
    ),
    fc!(
        "BE-MEM-002",
        Warning,
        "Memory allocation reduced",
        "The requested test buffer could not be allocated; coverage was reduced.",
        ["Insufficient free memory", "OS memory limits"],
        [
            "Close other applications",
            "Use preboot diagnostics for broad coverage"
        ]
    ),
    fc!(
        "BE-MEM-003",
        Error,
        "ECC errors reported",
        "The memory controller reported corrected or uncorrected ECC errors.",
        [
            "Degrading DIMM",
            "Memory controller",
            "Seating/contact issue"
        ],
        [
            "Identify the reporting channel/DIMM",
            "Reseat or replace the DIMM with power removed",
            "Repeat memory testing"
        ]
    ),
    fc!(
        "BE-STO-001",
        Error,
        "Scratch file integrity mismatch",
        "Data read back from the scratch test file did not match what was written.",
        [
            "Storage device error",
            "File system or driver issue",
            "Memory error",
            "Controller/cable issue"
        ],
        [
            "Review SMART/NVMe health",
            "Repeat on a different volume",
            "Run memory tests"
        ]
    ),
    fc!(
        "BE-STO-002",
        Error,
        "Storage health critical",
        "The device reported a critical health warning or failed self-assessment.",
        [
            "Worn or failing media",
            "Controller fault",
            "Over-temperature"
        ],
        [
            "Back up data",
            "Review SMART attributes",
            "Replace the device per service procedure"
        ]
    ),
    fc!(
        "BE-STO-003",
        Warning,
        "Insufficient scratch space",
        "Free space was insufficient for the requested scratch test size.",
        ["Volume full"],
        ["Free space or choose a dedicated scratch volume"]
    ),
    fc!(
        "BE-STO-004",
        Warning,
        "Storage error counters non-zero",
        "The device or kernel reported media/integrity errors.",
        ["Media degradation", "Link/cable errors"],
        ["Review error log entries", "Re-check after reseating"]
    ),
    fc!(
        "BE-GPU-001",
        Info,
        "GPU backend unavailable",
        "No supported compute/graphics backend is available for this device.",
        [
            "Driver or runtime not installed",
            "Backend not implemented for this vendor"
        ],
        [
            "Install the supported driver",
            "See the hardware support matrix"
        ]
    ),
    fc!(
        "BE-GPU-002",
        Info,
        "Only software Vulkan devices present",
        "The only Vulkan devices are CPU-emulated (e.g. Mesa llvmpipe); GPU hardware was not tested.",
        [
            "GPU driver without Vulkan support",
            "Virtual machine",
            "No GPU installed"
        ],
        [
            "Install the GPU vendor driver with Vulkan support",
            "Confirm the GPU is detected by the OS"
        ]
    ),
    fc!(
        "BE-GPU-010",
        Error,
        "GPU compute result mismatch",
        "A deterministic compute kernel produced results different from the CPU reference.",
        [
            "GPU core instability",
            "VRAM error",
            "Driver defect",
            "Unstable power delivery or overclock",
            "Thermal issue"
        ],
        [
            "Repeat the compute test on the same GPU",
            "Run the VRAM pattern test",
            "Check driver version and stock clocks",
            "Compare with a known-good GPU in the same slot"
        ]
    ),
    fc!(
        "BE-GPU-011",
        Error,
        "GPU device lost or hung",
        "The driver reported device loss, or GPU work did not complete in time.",
        [
            "Driver reset (TDR) or crash",
            "PCIe link/riser fault",
            "Insufficient or unstable power",
            "GPU hardware fault",
            "Overheating"
        ],
        [
            "Review system log for driver resets",
            "Inspect riser, seating and PCIe power connectors with power removed",
            "Repeat with the GPU isolated in another slot"
        ]
    ),
    fc!(
        "BE-GPU-012",
        Warning,
        "GPU memory coverage reduced",
        "The requested VRAM allocation could not be obtained; coverage was reduced.",
        [
            "Other processes using VRAM",
            "Driver allocation limits",
            "50% heap safety cap"
        ],
        [
            "Close other GPU applications",
            "Repeat with a smaller request and document coverage"
        ]
    ),
    fc!(
        "BE-GPU-014",
        Error,
        "GPU memory mismatch",
        "Data read back from a GPU memory allocation did not match the written pattern.",
        ["VRAM defect", "GPU core", "Driver", "Unstable power"],
        [
            "Repeat independent GPU test",
            "Review ECC counters",
            "Compare with a known-good configuration"
        ]
    ),
    fc!(
        "BE-GPU-020",
        Warning,
        "GPU telemetry incomplete",
        "Required GPU telemetry was not available from the supported interface.",
        ["Driver does not expose metric", "Unsupported GPU"],
        ["Document as unavailable; do not run high-load GPU profiles without required sensors"]
    ),
    fc!(
        "BE-PCIE-001",
        Warning,
        "PCIe link below capability",
        "The negotiated PCIe link speed or width is lower than the device's maximum.",
        [
            "Idle power-saving downshift (may be normal)",
            "Riser/slot signal integrity",
            "Slot wiring limits",
            "Device seating"
        ],
        [
            "Re-check under controlled load",
            "Compare against approved configuration manifest",
            "Inspect riser and seating with power removed"
        ]
    ),
    fc!(
        "BE-PCIE-002",
        Error,
        "PCIe link below manifest",
        "The negotiated link is below the value required by the approved configuration manifest.",
        ["Riser/cable", "Slot", "Device seating", "Firmware setting"],
        [
            "Hold unit",
            "Inspect riser and seating with power removed",
            "Repeat after reseat"
        ]
    ),
    fc!(
        "BE-PCIE-003",
        Warning,
        "PCIe AER errors reported",
        "Advanced Error Reporting counters are non-zero.",
        ["Signal integrity", "Riser", "Device fault"],
        [
            "Note the reporting device",
            "Repeat after reseat",
            "Escalate if uncorrectable errors present"
        ]
    ),
    fc!(
        "BE-GPU-021",
        Warning,
        "GPU clock throttling observed",
        "The driver reported active clock-throttle reasons (thermal, power or hardware slowdown) during the test.",
        [
            "Cooling insufficient or obstructed",
            "Power limit reached",
            "Hardware slowdown signal (PSU/board)",
            "Ambient temperature"
        ],
        [
            "Review thermal trend and fan operation",
            "Inspect GPU airflow and thermal interface",
            "Compare against approved power envelope"
        ]
    ),
    fc!(
        "BE-GPU-030",
        Error,
        "GPU rendering inconsistency",
        "Re-rendering an identical reference frame produced different pixels.",
        [
            "GPU core or VRAM instability",
            "Driver defect",
            "Overheating",
            "Power instability"
        ],
        [
            "Repeat the graphics test",
            "Run compute and VRAM tests",
            "Check temperatures and throttling during load"
        ]
    ),
    fc!(
        "BE-PCIE-004",
        Warning,
        "PCIe replay counter increased",
        "Link-level retransmissions (replays) increased during the test, indicating signal-integrity errors that were corrected.",
        [
            "Riser or cable signal integrity",
            "Slot contact",
            "Link speed beyond channel capability"
        ],
        [
            "Inspect and reseat the riser and GPU with power removed",
            "Repeat under load",
            "Compare against a known-good riser"
        ]
    ),
    fc!(
        "BE-THERM-001",
        Warning,
        "Temperature above warning limit",
        "A monitored temperature exceeded the profile warning limit.",
        [
            "Cooling obstruction",
            "Fan fault",
            "Thermal interface",
            "Ambient temperature"
        ],
        ["Inspect airflow and fans", "Review thermal trend"]
    ),
    fc!(
        "BE-THERM-002",
        Critical,
        "Temperature above critical limit; test aborted",
        "A monitored temperature exceeded the approved critical limit and the workload was stopped.",
        [
            "Cooling failure",
            "Fan fault",
            "Thermal interface",
            "Workload beyond approved envelope"
        ],
        [
            "Allow cooldown",
            "Hold unit",
            "Inspect cooling with power removed before retest"
        ]
    ),
    fc!(
        "BE-THERM-003",
        Warning,
        "Fan reports zero speed",
        "A fan tachometer reported 0 RPM while a reading was expected.",
        [
            "Stopped fan",
            "Disconnected fan",
            "Fan-stop mode at idle (may be normal)"
        ],
        ["Verify fan operation visually", "Check connector seating"]
    ),
    fc!(
        "BE-PWR-001",
        Error,
        "Unexpected shutdown or incomplete run",
        "A previous run did not complete and no terminal result was recorded.",
        [
            "Power delivery",
            "Thermal shutdown",
            "System hang",
            "Operator power-off"
        ],
        [
            "Review logs from the interrupted run",
            "Hold unit pending review"
        ]
    ),
    fc!(
        "BE-SNS-001",
        Critical,
        "Required sensor lost; test aborted",
        "A sensor required by the safety policy stopped reporting fresh values.",
        ["Driver issue", "Sensor fault", "Bus error"],
        [
            "Do not continue high-load testing",
            "Re-check sensor availability"
        ]
    ),
    fc!(
        "BE-SNS-002",
        Warning,
        "No temperature telemetry",
        "No temperature sensors were found through supported interfaces.",
        [
            "Missing driver/module",
            "Virtual machine",
            "Unsupported platform"
        ],
        [
            "Load supported sensor drivers",
            "High-load profiles remain blocked"
        ]
    ),
    fc!(
        "BE-NET-001",
        Warning,
        "Network interface errors",
        "Interface error counters are non-zero.",
        ["Cable", "Link partner", "NIC fault"],
        ["Replace cable", "Retest against a known-good peer"]
    ),
    fc!(
        "BE-NET-002",
        Warning,
        "Network link below expected",
        "The negotiated link speed is below the expected value.",
        ["Cable category", "Link partner", "Auto-negotiation"],
        ["Verify cable and peer capability"]
    ),
    fc!(
        "BE-FW-001",
        Warning,
        "Secure Boot disabled or unknown",
        "Secure Boot was not reported as enabled.",
        ["Firmware configuration", "Legacy boot mode"],
        ["Compare with approved firmware configuration; do not change settings automatically"]
    ),
    fc!(
        "BE-FW-002",
        Warning,
        "TPM not detected",
        "No TPM device was reported by the operating system.",
        ["TPM disabled in firmware", "Not present"],
        ["Compare with approved configuration"]
    ),
    fc!(
        "BE-CFG-001",
        Error,
        "Configuration mismatch",
        "Detected hardware does not match the approved configuration manifest.",
        [
            "Wrong part installed",
            "Device not detected",
            "Manifest error"
        ],
        [
            "Verify BOM",
            "Inspect seating",
            "Correct manifest only through approved change control"
        ]
    ),
    fc!(
        "BE-OS-001",
        Warning,
        "Hardware error events in system log",
        "The OS log contains hardware-related error events.",
        ["Various; see referenced events"],
        ["Review referenced events", "Correlate with tests"]
    ),
    fc!(
        "BE-OS-002",
        Info,
        "System log not accessible",
        "The system log could not be read with current privileges.",
        ["Insufficient privileges", "Log service unavailable"],
        ["Re-run with appropriate read permission"]
    ),
    fc!(
        "BE-SYS-001",
        Error,
        "Worker process crashed",
        "The isolated test worker terminated unexpectedly.",
        ["Software defect", "Memory error", "System instability"],
        [
            "Review worker logs",
            "Repeat test",
            "Run memory diagnostics"
        ]
    ),
    fc!(
        "BE-SYS-002",
        Error,
        "Worker timeout",
        "The test exceeded its time limit or stopped sending heartbeats.",
        ["System hang", "Severe throttling", "Software defect"],
        ["Review logs", "Repeat with monitoring"]
    ),
    fc!(
        "BE-SYS-003",
        Info,
        "Aborted by operator",
        "The operator stopped the test.",
        [],
        ["Re-run the test to obtain a result"]
    ),
    fc!(
        "BE-SYS-004",
        Error,
        "Prerequisite failed",
        "The test was not started because a prerequisite test did not pass.",
        ["Earlier failure"],
        ["Resolve the prerequisite failure first"]
    ),
    fc!(
        "BE-SAF-001",
        Info,
        "High-load test not authorized",
        "The test requires explicit authorization and an approved hardware safety profile.",
        [],
        ["Obtain authorization and an approved safety profile for this hardware"]
    ),
];

pub fn lookup(code: &str) -> Option<&'static FaultCode> {
    CATALOG.iter().find(|c| c.code == code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn codes_unique_and_well_formed() {
        let mut seen = HashSet::new();
        for c in CATALOG {
            assert!(seen.insert(c.code), "duplicate {}", c.code);
            assert!(c.code.starts_with("BE-"), "{}", c.code);
            let parts: Vec<_> = c.code.split('-').collect();
            assert_eq!(parts.len(), 3, "{}", c.code);
            assert_eq!(parts[2].len(), 3, "{}", c.code);
            assert!(!c.meaning.is_empty());
        }
    }

    #[test]
    fn lookup_works() {
        assert_eq!(lookup("BE-GPU-014").unwrap().title, "GPU memory mismatch");
        assert!(lookup("BE-XXX-999").is_none());
    }
}
