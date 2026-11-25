//! Read-only review tests. Each has a pure `evaluate_*` function operating
//! on typed inventory data so fault paths can be fixture-tested.

use crate::{DiagnosticTest, TestContext, desc, finish};
use be_core::inventory::{Inventory, SensorKind, SensorReading, format_bytes};
use be_core::model::*;
use be_core::status::TestStatus;
use serde_json::{Value, json};
use std::process::Command;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------- inventory

pub struct InventoryReview;

/// Compare inventory against an optional configuration manifest.
pub fn evaluate_inventory(
    inv: &Inventory,
    manifest: Option<&Value>,
    r: &mut TestResult,
) -> TestStatus {
    let mut status = TestStatus::Pass;
    match &inv.cpu {
        Some(c) => {
            r.measurements.push(Measurement::text(
                "CPU model",
                c.model.clone().unwrap_or("unknown".into()),
            ));
            r.measurements
                .push(Measurement::num("Logical CPUs", c.logical_cpus as f64, ""));
        }
        None => {
            r.measurements
                .push(Measurement::unavailable("CPU model", ""));
            status = TestStatus::Inconclusive;
        }
    }
    match &inv.memory {
        Some(m) => r.measurements.push(Measurement::num(
            "Installed memory (OS visible)",
            m.total_bytes as f64 / (1u64 << 30) as f64,
            "GiB",
        )),
        None => {
            r.measurements
                .push(Measurement::unavailable("Installed memory", "GiB"));
            status = TestStatus::Inconclusive;
        }
    }
    r.measurements.push(Measurement::num(
        "Storage devices",
        inv.storage.len() as f64,
        "",
    ));
    r.measurements
        .push(Measurement::num("GPUs", inv.gpus.len() as f64, ""));
    r.measurements
        .push(Measurement::num("PCI functions", inv.pci.len() as f64, ""));
    r.measurements
        .push(Measurement::num("Sensors", inv.sensors.len() as f64, ""));
    r.measurements.push(Measurement::num(
        "Unavailable items",
        inv.unavailable.len() as f64,
        "",
    ));
    for u in &inv.unavailable {
        r.notes
            .push(format!("Unavailable: {} — {}", u.item, u.reason));
    }

    let Some(m) = manifest.filter(|m| m.is_object()) else {
        r.coverage =
            "Inventory collected; no configuration manifest supplied (comparison not performed)"
                .into();
        return status;
    };
    let mut mismatches = Vec::new();
    let mut unknown = Vec::new();
    if let Some(want) = m.get("cpu_model_contains").and_then(|v| v.as_str()) {
        match inv.cpu.as_ref().and_then(|c| c.model.as_ref()) {
            Some(model) if model.to_lowercase().contains(&want.to_lowercase()) => {}
            Some(model) => {
                mismatches.push(format!("CPU model `{model}` does not contain `{want}`"))
            }
            None => unknown.push("CPU model".to_string()),
        }
    }
    if let Some(want) = m.get("min_logical_cpus").and_then(|v| v.as_u64()) {
        match &inv.cpu {
            Some(c) if c.logical_cpus as u64 >= want => {}
            Some(c) => mismatches.push(format!("{} logical CPUs < {want}", c.logical_cpus)),
            None => unknown.push("logical CPU count".into()),
        }
    }
    if let Some(want) = m.get("min_memory_gib").and_then(|v| v.as_f64()) {
        match &inv.memory {
            // OS-visible memory is slightly below installed capacity; allow 5%.
            Some(mem) if mem.total_bytes as f64 >= want * 0.95 * (1u64 << 30) as f64 => {}
            Some(mem) => mismatches.push(format!(
                "{} visible < {want} GiB",
                format_bytes(mem.total_bytes)
            )),
            None => unknown.push("memory size".into()),
        }
    }
    if let Some(want) = m.get("gpu_count").and_then(|v| v.as_u64())
        && inv.gpus.len() as u64 != want
    {
        mismatches.push(format!(
            "{} GPU(s) detected, manifest requires {want}",
            inv.gpus.len()
        ));
    }
    if let Some(want) = m.get("min_storage_devices").and_then(|v| v.as_u64())
        && (inv.storage.len() as u64) < want
    {
        mismatches.push(format!("{} storage device(s) < {want}", inv.storage.len()));
    }
    if let Some(want) = m.get("min_network_speed_mbps").and_then(|v| v.as_i64())
        && !inv
            .network
            .iter()
            .any(|n| n.speed_mbps.map(|s| s >= want).unwrap_or(false))
    {
        mismatches.push(format!("no interface with link >= {want} Mb/s"));
    }
    r.coverage = format!(
        "Inventory compared against manifest `{}`",
        m.get("name").and_then(|v| v.as_str()).unwrap_or("unnamed")
    );
    for mm in &mismatches {
        r.faults.push(FaultRecord::new(
            "BE-CFG-001",
            "Configuration mismatch",
            mm.clone(),
        ));
    }
    for u in &unknown {
        r.notes
            .push(format!("Manifest item could not be verified: {u}"));
    }
    if !mismatches.is_empty() {
        TestStatus::Fail
    } else if !unknown.is_empty() {
        TestStatus::Inconclusive
    } else {
        status
    }
}

impl DiagnosticTest for InventoryReview {
    fn descriptor(&self) -> TestDescriptor {
        desc(
            "BE-T-INV-001",
            "1.0.0",
            "Hardware Inventory & Manifest Check",
            Subsystem::System,
            LoadClass::ReadOnly,
            Privilege::ElevatedOptional,
            3,
            "Collects native inventory and, when a configuration manifest is supplied, compares CPU, memory, GPU, storage and network against it.",
            "Inventory snapshot, unavailable items, manifest mismatches",
            "Some identity fields (serial numbers) need elevated privileges. Without a manifest only collection is verified.",
            json!({"manifest": null}),
        )
    }
    fn run(&self, ctx: &TestContext) -> TestResult {
        let started = Instant::now();
        let mut r = TestResult::new(&self.descriptor(), "System", ctx.provenance);
        (ctx.progress)(None, "collecting inventory");
        let inv = ctx.platform.inventory();
        let st = evaluate_inventory(&inv, ctx.params.get("manifest"), &mut r);
        finish(r, started, st)
    }
}

// ------------------------------------------------------------------ sensors

pub struct SensorSurvey;

/// Evaluate two sensor samples taken `interval` apart.
pub fn evaluate_sensors(
    a: &[SensorReading],
    b: &[SensorReading],
    r: &mut TestResult,
) -> TestStatus {
    let temps: Vec<&SensorReading> = b
        .iter()
        .filter(|s| s.kind == SensorKind::Temperature)
        .collect();
    let readable = b.iter().filter(|s| s.value.is_some()).count();
    r.measurements
        .push(Measurement::num("Sensors enumerated", b.len() as f64, ""));
    r.measurements
        .push(Measurement::num("Sensors readable", readable as f64, ""));
    r.measurements.push(Measurement::num(
        "Temperature sensors",
        temps.len() as f64,
        "",
    ));
    let mut status = TestStatus::Pass;
    let lost: Vec<&str> = a
        .iter()
        .filter(|s| s.value.is_some())
        .filter(|s| !b.iter().any(|x| x.id == s.id && x.value.is_some()))
        .map(|s| s.id.as_str())
        .collect();
    for id in &lost {
        r.faults.push(FaultRecord::new(
            "BE-SNS-001",
            "Sensor stopped reporting",
            format!("{id} readable in first sample but not second"),
        ));
        status = TestStatus::Warning;
    }
    for s in &temps {
        match (s.value, s.crit) {
            (Some(v), Some(c)) if v >= c => {
                r.faults.push(
                    FaultRecord::new(
                        "BE-THERM-002",
                        "Temperature at or above critical",
                        format!("{} {} = {v:.1} °C (crit {c:.1})", s.chip, s.label),
                    )
                    .on(s.id.clone()),
                );
                status = TestStatus::Fail;
            }
            (Some(v), _) => {
                r.measurements
                    .push(Measurement::num(format!("{} {}", s.chip, s.label), v, "°C"))
            }
            (None, _) => r.measurements.push(Measurement::unavailable(
                format!("{} {}", s.chip, s.label),
                "°C",
            )),
        }
    }
    for s in b.iter().filter(|s| s.kind == SensorKind::Fan) {
        if s.value == Some(0.0) {
            r.faults.push(FaultRecord::new(
                "BE-THERM-003",
                "Fan reports 0 RPM",
                format!("{} {}", s.chip, s.label),
            ));
            if status == TestStatus::Pass {
                status = TestStatus::Warning;
            }
        }
    }
    if temps.is_empty() {
        r.faults.push(FaultRecord::new(
            "BE-SNS-002",
            "No temperature telemetry",
            "No temperature sensors found through supported interfaces",
        ));
        if status == TestStatus::Pass {
            status = TestStatus::Warning;
        }
    }
    r.coverage = format!(
        "{} sensors sampled twice; values reported as read, unavailable values are not substituted",
        b.len()
    );
    status
}

impl DiagnosticTest for SensorSurvey {
    fn descriptor(&self) -> TestDescriptor {
        desc(
            "BE-T-SNS-001",
            "1.0.0",
            "Sensor Survey & Freshness",
            Subsystem::Sensors,
            LoadClass::ReadOnly,
            Privilege::Standard,
            3,
            "Enumerates temperature, fan, voltage, power and clock sensors, samples twice, flags lost sensors, critical temperatures and stopped fans.",
            "Sensor counts, readings with units, missing telemetry",
            "Availability depends on kernel drivers / vendor interfaces. Missing temperature telemetry blocks high-load profiles.",
            json!({"interval_ms": 1000}),
        )
    }
    fn run(&self, ctx: &TestContext) -> TestResult {
        let started = Instant::now();
        let mut r = TestResult::new(&self.descriptor(), "All sensors", ctx.provenance);
        let a = ctx.platform.sample_sensors();
        (ctx.progress)(Some(0.5), "sampling sensors");
        std::thread::sleep(Duration::from_millis(
            ctx.p_u64("interval_ms", 1000).min(10_000),
        ));
        let b = ctx.platform.sample_sensors();
        let st = evaluate_sensors(&a, &b, &mut r);
        finish(r, started, st)
    }
}

// --------------------------------------------------------------- error logs

pub struct HardwareErrorLog;

const HW_PATTERNS: &[(&str, &str)] = &[
    ("machine check", "BE-CPU-003"),
    ("mce:", "BE-CPU-003"),
    ("hardware error", "BE-CPU-003"),
    ("edac", "BE-MEM-003"),
    ("pcie bus error", "BE-PCIE-003"),
    ("aer:", "BE-PCIE-003"),
    ("i/o error", "BE-STO-004"),
    ("nvme", "BE-STO-004"),
    ("ata", "BE-STO-004"),
    ("thermal", "BE-THERM-001"),
    ("throttl", "BE-THERM-001"),
    ("gpu has fallen off", "BE-OS-001"),
    ("xid", "BE-OS-001"),
    ("amdgpu", "BE-OS-001"),
];

fn classify(msg: &str) -> Option<&'static str> {
    let l = msg.to_ascii_lowercase();
    // Only consider error-ish lines for broad keywords.
    for (pat, code) in HW_PATTERNS {
        if l.contains(pat) {
            let broad = matches!(*pat, "nvme" | "ata" | "amdgpu" | "thermal");
            if broad
                && !(l.contains("error")
                    || l.contains("fail")
                    || l.contains("timeout")
                    || l.contains("reset")
                    || l.contains("critical"))
            {
                continue;
            }
            return Some(code);
        }
    }
    None
}

/// Evaluate kernel log lines (already filtered to priority <= warning).
pub fn evaluate_log_lines(
    lines: &[String],
    ecc: &[(String, Option<u64>, Option<u64>)],
    r: &mut TestResult,
) -> TestStatus {
    let mut status = TestStatus::Pass;
    let mut hits = 0;
    for l in lines {
        if let Some(code) = classify(l) {
            hits += 1;
            if r.faults.len() < 32 {
                r.faults.push(FaultRecord::new(
                    code,
                    "Hardware-related system log event",
                    l.chars().take(300).collect::<String>(),
                ));
            }
            status = TestStatus::Warning;
        }
    }
    r.measurements.push(Measurement::num(
        "Log entries reviewed",
        lines.len() as f64,
        "",
    ));
    r.measurements
        .push(Measurement::num("Hardware-related events", hits as f64, ""));
    for (name, ce, ue) in ecc {
        match ce {
            Some(c) => r.measurements.push(Measurement::num(
                format!("{name} corrected ECC"),
                *c as f64,
                "",
            )),
            None => r.measurements.push(Measurement::unavailable(
                format!("{name} corrected ECC"),
                "",
            )),
        }
        if let Some(u) = ue {
            r.measurements.push(Measurement::num(
                format!("{name} uncorrected ECC"),
                *u as f64,
                "",
            ));
            if *u > 0 {
                r.faults.push(FaultRecord::new(
                    "BE-MEM-003",
                    "Uncorrected ECC errors",
                    format!("{name}: {u}"),
                ));
                status = TestStatus::Fail;
            }
        }
        if ce.unwrap_or(0) > 0 {
            r.faults.push(FaultRecord::new(
                "BE-MEM-003",
                "Corrected ECC errors",
                format!("{name}: {}", ce.unwrap()),
            ));
            if status == TestStatus::Pass {
                status = TestStatus::Warning;
            }
        }
    }
    status
}

/// Read kernel log messages of priority warning or worse for the current
/// boot. Returns Err(reason) if the log is not accessible.
pub fn read_kernel_log() -> Result<Vec<String>, String> {
    #[cfg(target_os = "linux")]
    {
        let out = Command::new("journalctl")
            .args(["-k", "-b", "-p", "warning", "-o", "cat", "--no-pager", "-q"])
            .output()
            .map_err(|e| format!("journalctl unavailable: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "journalctl exited with {}: {}",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let lines: Vec<String> = text
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.starts_with("-- "))
            .map(String::from)
            .collect();
        if lines.is_empty() {
            // journalctl may succeed but have no kernel messages if the
            // journal is not persistent or kernel logging is not captured.
            let probe = Command::new("journalctl")
                .args(["-k", "-b", "-n", "1", "-o", "cat", "--no-pager", "-q"])
                .output();
            if probe.map(|o| o.stdout.is_empty()).unwrap_or(true) {
                return Err("journal contains no kernel messages for this boot (journal not capturing kernel log, or insufficient permission)".into());
            }
        }
        Ok(lines)
    }
    #[cfg(windows)]
    {
        let script = "Get-WinEvent -FilterHashtable @{LogName='System'; Level=1,2,3; StartTime=(Get-CimInstance Win32_OperatingSystem).LastBootUpTime} -MaxEvents 2000 -ErrorAction SilentlyContinue | ForEach-Object { $_.ProviderName + ': ' + ($_.Message -replace '\\s+',' ') }";
        let out = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .output()
            .map_err(|e| format!("PowerShell unavailable: {e}"))?;
        Ok(String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(String::from)
            .collect())
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        Err("System log review not implemented for this OS".into())
    }
}

impl DiagnosticTest for HardwareErrorLog {
    fn descriptor(&self) -> TestDescriptor {
        desc(
            "BE-T-OS-001",
            "1.0.0",
            "Hardware Error Log Review",
            Subsystem::Os,
            LoadClass::ReadOnly,
            Privilege::ElevatedOptional,
            3,
            "Reviews kernel/system log events since boot (Linux journal; Windows System log incl. WHEA) for machine-check, ECC, PCIe AER, storage and thermal events, and reads EDAC ECC counters.",
            "Matching log lines, ECC counters",
            "Keyword classification is heuristic and lists events for review; it does not diagnose components. Access may need adm/systemd-journal group or administrator.",
            json!({}),
        )
    }
    fn run(&self, ctx: &TestContext) -> TestResult {
        let started = Instant::now();
        let mut r = TestResult::new(&self.descriptor(), "Operating system logs", ctx.provenance);
        let inv = ctx.platform.inventory();
        let ecc: Vec<_> = inv
            .memory
            .as_ref()
            .map(|m| {
                m.ecc_controllers
                    .iter()
                    .map(|c| (c.name.clone(), c.corrected, c.uncorrected))
                    .collect()
            })
            .unwrap_or_default();
        match read_kernel_log() {
            Ok(lines) => {
                let st = evaluate_log_lines(&lines, &ecc, &mut r);
                r.coverage = format!(
                    "{} warning-or-worse kernel/system log entries since boot",
                    lines.len()
                );
                finish(r, started, st)
            }
            Err(e) => {
                r.faults.push(FaultRecord::new(
                    "BE-OS-002",
                    "System log not accessible",
                    e,
                ));
                let st = evaluate_log_lines(&[], &ecc, &mut r);
                r.coverage = "System log not readable; ECC counters only".into();
                finish(
                    r,
                    started,
                    if st == TestStatus::Fail {
                        TestStatus::Fail
                    } else {
                        TestStatus::Inconclusive
                    },
                )
            }
        }
    }
}

// --------------------------------------------------------------------- PCIe

pub struct PcieLinkReview;

pub fn parse_gts(s: &str) -> Option<f64> {
    s.split_whitespace().next().and_then(|x| x.parse().ok())
}

pub fn evaluate_pcie(inv: &Inventory, expected: Option<&Value>, r: &mut TestResult) -> TestStatus {
    let mut status = TestStatus::Pass;
    let with_link: Vec<_> = inv
        .pci
        .iter()
        .filter(|d| d.max_link_width.is_some() && d.max_link_speed.is_some())
        .collect();
    r.measurements.push(Measurement::num(
        "PCIe functions with link data",
        with_link.len() as f64,
        "",
    ));
    if with_link.is_empty()
        && expected
            .and_then(|e| e.as_array())
            .map(|a| a.is_empty())
            .unwrap_or(true)
    {
        r.notes.push(
            "No PCIe link status exposed by this platform (common in virtual machines)".into(),
        );
        return TestStatus::Unsupported;
    }
    for d in &with_link {
        let cur_w = d.current_link_width;
        let cur_s = d.current_link_speed.as_deref().and_then(parse_gts);
        let max_w = d.max_link_width;
        let max_s = d.max_link_speed.as_deref().and_then(parse_gts);
        r.measurements.push(Measurement::text(
            format!("{} link", d.address),
            format!(
                "x{} @ {} (max x{} @ {})",
                cur_w.map(|w| w.to_string()).unwrap_or("?".into()),
                d.current_link_speed.clone().unwrap_or("unknown".into()),
                max_w.map(|w| w.to_string()).unwrap_or("?".into()),
                d.max_link_speed.clone().unwrap_or("unknown".into())
            ),
        ));
        let down_w = matches!((cur_w, max_w), (Some(c), Some(m)) if c < m);
        let down_s = matches!((cur_s, max_s), (Some(c), Some(m)) if c + 1e-9 < m);
        if down_w || down_s {
            r.faults.push(FaultRecord::new("BE-PCIE-001", "Link below device maximum", format!("{} ({}) negotiated below its maximum; may be idle power saving or slot limit", d.address, d.class_name)).on(d.address.clone()));
        }
        if d.aer_fatal.unwrap_or(0) > 0 || d.aer_nonfatal.unwrap_or(0) > 0 {
            r.faults.push(
                FaultRecord::new(
                    "BE-PCIE-003",
                    "Uncorrectable AER errors",
                    format!(
                        "{}: nonfatal {:?}, fatal {:?}",
                        d.address, d.aer_nonfatal, d.aer_fatal
                    ),
                )
                .on(d.address.clone()),
            );
            status = TestStatus::Fail;
        } else if d.aer_correctable.unwrap_or(0) > 0 {
            r.faults.push(
                FaultRecord::new(
                    "BE-PCIE-003",
                    "Correctable AER errors",
                    format!("{}: {:?}", d.address, d.aer_correctable),
                )
                .on(d.address.clone()),
            );
            if status == TestStatus::Pass {
                status = TestStatus::Warning;
            }
        }
    }
    // Manifest expectations are authoritative; downshift alone is informational.
    if let Some(list) = expected.and_then(|e| e.as_array()) {
        for e in list {
            let addr = e.get("address").and_then(|v| v.as_str()).unwrap_or("");
            let Some(d) = inv.pci.iter().find(|d| d.address == addr) else {
                r.faults.push(FaultRecord::new(
                    "BE-PCIE-002",
                    "Expected PCIe device missing",
                    format!("{addr} not present"),
                ));
                status = TestStatus::Fail;
                continue;
            };
            if let Some(w) = e.get("min_width").and_then(|v| v.as_u64())
                && d.current_link_width.map(|c| (c as u64) < w).unwrap_or(true)
            {
                r.faults.push(
                    FaultRecord::new(
                        "BE-PCIE-002",
                        "Link width below manifest",
                        format!("{addr}: x{:?} < x{w}", d.current_link_width),
                    )
                    .on(addr),
                );
                status = TestStatus::Fail;
            }
            if let Some(s) = e.get("min_speed_gts").and_then(|v| v.as_f64())
                && d.current_link_speed
                    .as_deref()
                    .and_then(parse_gts)
                    .map(|c| c + 1e-9 < s)
                    .unwrap_or(true)
            {
                r.faults.push(
                    FaultRecord::new(
                        "BE-PCIE-002",
                        "Link speed below manifest",
                        format!("{addr}: {:?} < {s} GT/s", d.current_link_speed),
                    )
                    .on(addr),
                );
                status = TestStatus::Fail;
            }
        }
    }
    r.coverage = format!(
        "{} PCIe links reviewed at time of test (idle state unless run under load)",
        with_link.len()
    );
    status
}

impl DiagnosticTest for PcieLinkReview {
    fn descriptor(&self) -> TestDescriptor {
        desc(
            "BE-T-PCIE-001",
            "1.0.0",
            "PCIe Link & AER Review",
            Subsystem::Pcie,
            LoadClass::ReadOnly,
            Privilege::Standard,
            1,
            "Compares negotiated PCIe link width/speed against device maxima and manifest expectations; reviews AER error counters.",
            "Per-link width/speed, AER counters",
            "Links commonly downshift at idle; only manifest violations fail. Windows link data not yet implemented.",
            json!({"expected": []}),
        )
    }
    fn run(&self, ctx: &TestContext) -> TestResult {
        let started = Instant::now();
        let mut r = TestResult::new(&self.descriptor(), "PCIe", ctx.provenance);
        let inv = ctx.platform.inventory();
        let st = evaluate_pcie(&inv, ctx.params.get("expected"), &mut r);
        finish(r, started, st)
    }
}

// ------------------------------------------------------------------ network

pub struct NetworkLinkReview;

pub fn evaluate_network(
    inv: &Inventory,
    expected_mbps: Option<i64>,
    r: &mut TestResult,
) -> TestStatus {
    let phys: Vec<_> = inv.network.iter().filter(|n| !n.is_virtual).collect();
    r.measurements.push(Measurement::num(
        "Physical interfaces",
        phys.len() as f64,
        "",
    ));
    if phys.is_empty() {
        r.notes
            .push("No physical network interfaces detected".into());
        return TestStatus::Unsupported;
    }
    let mut status = TestStatus::Pass;
    for n in &phys {
        match n.speed_mbps {
            Some(s) => r.measurements.push(Measurement::num(
                format!("{} link speed", n.name),
                s as f64,
                "Mb/s",
            )),
            None => r.measurements.push(Measurement::unavailable(
                format!("{} link speed", n.name),
                "Mb/s",
            )),
        }
        let errs = n.rx_errors.unwrap_or(0) + n.tx_errors.unwrap_or(0);
        if errs > 0 {
            r.faults.push(
                FaultRecord::new(
                    "BE-NET-001",
                    "Interface error counters non-zero",
                    format!("{}: rx {:?}, tx {:?}", n.name, n.rx_errors, n.tx_errors),
                )
                .on(n.name.clone()),
            );
            status = TestStatus::Warning;
        }
    }
    if let Some(want) = expected_mbps
        && !phys
            .iter()
            .any(|n| n.speed_mbps.map(|s| s >= want).unwrap_or(false))
    {
        r.faults.push(FaultRecord::new(
            "BE-NET-002",
            "No link at expected speed",
            format!("expected >= {want} Mb/s"),
        ));
        status = TestStatus::Fail;
    }
    r.coverage = "Link state and error counters only; no traffic generated (throughput test requires authorized peer)".into();
    status
}

impl DiagnosticTest for NetworkLinkReview {
    fn descriptor(&self) -> TestDescriptor {
        desc(
            "BE-T-NET-001",
            "1.0.0",
            "Network Link Review",
            Subsystem::Network,
            LoadClass::ReadOnly,
            Privilege::Standard,
            1,
            "Reviews physical interface link state, negotiated speed and error counters. Generates no network traffic.",
            "Link speed, error counters",
            "10GbE throughput validation requires an authorized iperf3-capable peer (not implemented).",
            json!({"expected_mbps": null}),
        )
    }
    fn run(&self, ctx: &TestContext) -> TestResult {
        let started = Instant::now();
        let mut r = TestResult::new(&self.descriptor(), "Network interfaces", ctx.provenance);
        let inv = ctx.platform.inventory();
        let st = evaluate_network(
            &inv,
            ctx.params.get("expected_mbps").and_then(|v| v.as_i64()),
            &mut r,
        );
        finish(r, started, st)
    }
}

// ----------------------------------------------------------------- firmware

pub struct FirmwareSecurityReview;

pub fn evaluate_firmware(
    inv: &Inventory,
    require_sb: bool,
    require_tpm: bool,
    r: &mut TestResult,
) -> TestStatus {
    let fw = &inv.firmware;
    let mut status = TestStatus::Pass;
    let opt = |b: Option<bool>| match b {
        Some(true) => "yes",
        Some(false) => "no",
        None => "unknown",
    };
    r.measurements.push(Measurement::text(
        "BIOS version",
        fw.bios_version.clone().unwrap_or("unknown".into()),
    ));
    r.measurements
        .push(Measurement::text("UEFI boot", opt(fw.uefi)));
    r.measurements
        .push(Measurement::text("Secure Boot", opt(fw.secure_boot)));
    r.measurements
        .push(Measurement::text("TPM present", opt(fw.tpm_present)));
    if let Some(v) = &fw.tpm_version {
        r.measurements
            .push(Measurement::text("TPM version", v.clone()));
    }
    let mut check = |val: Option<bool>, required: bool, code: &str, what: &str| match val {
        Some(true) => {}
        Some(false) => {
            r.faults.push(FaultRecord::new(
                code,
                format!("{what} not enabled/present"),
                format!("{what} reported as disabled/absent"),
            ));
            status = if required {
                TestStatus::Fail
            } else if status == TestStatus::Pass {
                TestStatus::Warning
            } else {
                status
            };
        }
        None => {
            r.notes.push(format!("{what} state unknown"));
            if required && status != TestStatus::Fail {
                status = TestStatus::Inconclusive;
            }
        }
    };
    check(fw.secure_boot, require_sb, "BE-FW-001", "Secure Boot");
    check(fw.tpm_present, require_tpm, "BE-FW-002", "TPM");
    r.coverage = "Read-only firmware/security state; no settings changed".into();
    status
}

impl DiagnosticTest for FirmwareSecurityReview {
    fn descriptor(&self) -> TestDescriptor {
        desc(
            "BE-T-FW-001",
            "1.0.0",
            "Firmware & Security Review",
            Subsystem::Firmware,
            LoadClass::ReadOnly,
            Privilege::ElevatedOptional,
            1,
            "Reads BIOS/UEFI version, UEFI boot mode, Secure Boot and TPM presence and compares with profile requirements. Never changes firmware settings.",
            "Firmware version, Secure Boot state, TPM presence/version",
            "Some values require elevated privileges to read and are reported as unknown otherwise.",
            json!({"require_secure_boot": false, "require_tpm": false}),
        )
    }
    fn run(&self, ctx: &TestContext) -> TestResult {
        let started = Instant::now();
        let mut r = TestResult::new(&self.descriptor(), "Firmware", ctx.provenance);
        let inv = ctx.platform.inventory();
        let st = evaluate_firmware(
            &inv,
            ctx.p_bool("require_secure_boot", false),
            ctx.p_bool("require_tpm", false),
            &mut r,
        );
        finish(r, started, st)
    }
}

#[cfg(test)]
mod tests {
    //! FIXTURE TESTS over synthetic inventories.
    use super::*;
    use be_core::inventory::*;
    use be_core::time::UtcMillis;

    fn result() -> TestResult {
        TestResult::not_executed("X", "1", TestStatus::Running, "", Provenance::Fixture)
    }

    fn inv() -> Inventory {
        Inventory {
            cpu: Some(CpuInfo {
                model: Some("Fixture CPU 9000".into()),
                logical_cpus: 32,
                ..Default::default()
            }),
            memory: Some(MemoryInfo {
                total_bytes: 63 * (1 << 30),
                ..Default::default()
            }),
            gpus: vec![GpuDevice::default(), GpuDevice::default()],
            storage: vec![StorageDevice::default()],
            ..Default::default()
        }
    }

    #[test]
    fn manifest_match_and_mismatch() {
        let m = json!({"cpu_model_contains":"9000","min_memory_gib":64,"gpu_count":2,"min_storage_devices":1});
        let mut r = result();
        assert_eq!(
            evaluate_inventory(&inv(), Some(&m), &mut r),
            TestStatus::Pass,
            "{:?}",
            r.faults
        );
        let m = json!({"gpu_count":1,"min_logical_cpus":64});
        let mut r = result();
        assert_eq!(
            evaluate_inventory(&inv(), Some(&m), &mut r),
            TestStatus::Fail
        );
        assert_eq!(r.faults.len(), 2);
        assert!(r.faults.iter().all(|f| f.code == "BE-CFG-001"));
        let mut r = result();
        let empty = Inventory::default();
        assert_eq!(
            evaluate_inventory(&empty, Some(&json!({"cpu_model_contains":"x"})), &mut r),
            TestStatus::Inconclusive
        );
    }

    fn sensor(id: &str, kind: SensorKind, v: Option<f64>, crit: Option<f64>) -> SensorReading {
        SensorReading {
            id: id.into(),
            chip: "c".into(),
            label: id.into(),
            kind,
            value: v,
            unit: kind.unit().into(),
            crit,
            max: None,
            ts: UtcMillis(0),
            source: "fixture".into(),
        }
    }

    #[test]
    fn sensor_paths() {
        let mut r = result();
        assert_eq!(evaluate_sensors(&[], &[], &mut r), TestStatus::Warning);
        assert_eq!(r.faults[0].code, "BE-SNS-002");

        let a = vec![sensor(
            "t1",
            SensorKind::Temperature,
            Some(50.0),
            Some(100.0),
        )];
        let mut r = result();
        assert_eq!(evaluate_sensors(&a, &a, &mut r), TestStatus::Pass);

        let hot = vec![sensor(
            "t1",
            SensorKind::Temperature,
            Some(101.0),
            Some(100.0),
        )];
        let mut r = result();
        assert_eq!(evaluate_sensors(&a, &hot, &mut r), TestStatus::Fail);

        let lost = vec![sensor("t1", SensorKind::Temperature, None, Some(100.0))];
        let mut r = result();
        let st = evaluate_sensors(&a, &lost, &mut r);
        assert_eq!(st, TestStatus::Warning);
        assert!(r.faults.iter().any(|f| f.code == "BE-SNS-001"));
        assert!(
            r.measurements
                .iter()
                .any(|m| m.quality == DataQuality::Unavailable)
        );

        let fan = vec![
            sensor("t1", SensorKind::Temperature, Some(40.0), None),
            sensor("f", SensorKind::Fan, Some(0.0), None),
        ];
        let mut r = result();
        assert_eq!(evaluate_sensors(&fan, &fan, &mut r), TestStatus::Warning);
    }

    #[test]
    fn log_classification() {
        let lines: Vec<String> = vec![
            "mce: [Hardware Error]: Machine check events logged".into(),
            "pcieport 0000:00:01.0: AER: Corrected error received".into(),
            "nvme nvme0: I/O 12 QID 3 timeout, aborting".into(),
            "nvme nvme0: 4/0/0 default/read/poll queues".into(),
            "random warning about usb".into(),
        ];
        let mut r = result();
        assert_eq!(evaluate_log_lines(&lines, &[], &mut r), TestStatus::Warning);
        let codes: Vec<_> = r.faults.iter().map(|f| f.code.as_str()).collect();
        assert_eq!(codes, ["BE-CPU-003", "BE-PCIE-003", "BE-STO-004"]);
        let mut r = result();
        assert_eq!(
            evaluate_log_lines(&[], &[("mc0".into(), Some(0), Some(1))], &mut r),
            TestStatus::Fail
        );
        let mut r = result();
        assert_eq!(evaluate_log_lines(&[], &[], &mut r), TestStatus::Pass);
    }

    #[test]
    fn pcie_paths() {
        let mut i = Inventory::default();
        let mut r = result();
        assert_eq!(evaluate_pcie(&i, None, &mut r), TestStatus::Unsupported);
        i.pci.push(PciDevice {
            address: "0000:01:00.0".into(),
            class_name: "VGA".into(),
            current_link_speed: Some("8.0 GT/s PCIe".into()),
            current_link_width: Some(8),
            max_link_speed: Some("32.0 GT/s PCIe".into()),
            max_link_width: Some(16),
            ..Default::default()
        });
        let mut r = result();
        assert_eq!(
            evaluate_pcie(&i, None, &mut r),
            TestStatus::Pass,
            "idle downshift alone is informational"
        );
        assert_eq!(r.faults[0].code, "BE-PCIE-001");
        let exp = json!([{"address":"0000:01:00.0","min_width":16,"min_speed_gts":16.0},{"address":"0000:02:00.0"}]);
        let mut r = result();
        assert_eq!(evaluate_pcie(&i, Some(&exp), &mut r), TestStatus::Fail);
        assert_eq!(
            r.faults.iter().filter(|f| f.code == "BE-PCIE-002").count(),
            3
        );
        i.pci[0].aer_correctable = Some(2);
        let mut r = result();
        assert_eq!(evaluate_pcie(&i, None, &mut r), TestStatus::Warning);
        i.pci[0].aer_fatal = Some(1);
        let mut r = result();
        assert_eq!(evaluate_pcie(&i, None, &mut r), TestStatus::Fail);
    }

    #[test]
    fn network_and_firmware_paths() {
        let mut i = Inventory::default();
        let mut r = result();
        assert_eq!(evaluate_network(&i, None, &mut r), TestStatus::Unsupported);
        i.network.push(NetworkInterface {
            name: "eth0".into(),
            speed_mbps: Some(1000),
            rx_errors: Some(0),
            tx_errors: Some(0),
            ..Default::default()
        });
        let mut r = result();
        assert_eq!(evaluate_network(&i, Some(10000), &mut r), TestStatus::Fail);
        let mut r = result();
        assert_eq!(evaluate_network(&i, Some(1000), &mut r), TestStatus::Pass);

        let mut r = result();
        assert_eq!(
            evaluate_firmware(&i, true, false, &mut r),
            TestStatus::Inconclusive
        );
        i.firmware.secure_boot = Some(false);
        i.firmware.tpm_present = Some(true);
        let mut r = result();
        assert_eq!(
            evaluate_firmware(&i, false, true, &mut r),
            TestStatus::Warning
        );
        let mut r = result();
        assert_eq!(evaluate_firmware(&i, true, true, &mut r), TestStatus::Fail);
    }
}
