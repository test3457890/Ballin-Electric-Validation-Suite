//! GPU tests backed by the Vulkan engine (`be-gpu`).
//!
//! * Device selection: hardware devices only by default. Software Vulkan
//!   devices (Mesa llvmpipe/lavapipe) are excluded unless
//!   `include_software` is set (used by automated tests); results on them
//!   are labelled and never describe GPU hardware.
//! * `mode = "independent"` tests one device at a time; `"simultaneous"`
//!   runs every selected device concurrently (multi-GPU power/thermal load).
//! * Vendor telemetry (NVML, amdgpu sysfs) is sampled during each test:
//!   temperature, power, clocks, throttle reasons and PCIe replay counters.

use crate::{DiagnosticTest, TestContext, desc, finish};
use be_core::inventory::{Inventory, SensorKind, format_bytes};
use be_core::model::*;
use be_core::status::TestStatus;
use be_gpu::DeviceInfo;
use be_platform::gpu::GpuStatus;
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ------------------------------------------------------------ inventory

pub struct GpuInventory;

pub fn evaluate_gpus(
    inv: &Inventory,
    vk: &Result<Vec<DeviceInfo>, String>,
    expected_count: Option<u64>,
    r: &mut TestResult,
) -> TestStatus {
    r.measurements.push(Measurement::num(
        "GPUs detected (PCI)",
        inv.gpus.len() as f64,
        "",
    ));
    let mut status = TestStatus::Pass;
    match vk {
        Ok(devs) => {
            let hw = devs.iter().filter(|d| !d.software).count();
            r.measurements
                .push(Measurement::num("Vulkan hardware devices", hw as f64, ""));
            for d in devs {
                r.measurements.push(Measurement::text(
                    format!("Vulkan {} {}", d.index, d.name),
                    format!(
                        "{} {} | {} | VRAM heap {} | API {} | PCI {}",
                        d.vendor,
                        d.device_type,
                        if d.software {
                            "SOFTWARE device (not GPU hardware)"
                        } else {
                            "hardware"
                        },
                        format_bytes(d.device_local_bytes),
                        d.api_version,
                        d.pci_address.clone().unwrap_or("unknown".into())
                    ),
                ));
            }
        }
        Err(e) => r.notes.push(format!("Vulkan not available: {e}")),
    }
    if inv.gpus.is_empty() {
        if expected_count.unwrap_or(0) > 0 {
            r.faults.push(FaultRecord::new(
                "BE-CFG-001",
                "Expected GPU not detected",
                format!("manifest requires {}", expected_count.unwrap()),
            ));
            return TestStatus::Fail;
        }
        r.notes
            .push("No display/3D controllers found on the PCI bus".into());
        return TestStatus::Unsupported;
    }
    for (i, g) in inv.gpus.iter().enumerate() {
        let name = g.name.clone().unwrap_or("unknown".into());
        r.measurements.push(Measurement::text(
            format!("GPU {i} ({})", g.id),
            format!("{} {}", g.vendor.clone().unwrap_or_default(), name),
        ));
        match g.vram_total_bytes {
            Some(v) => r
                .measurements
                .push(Measurement::text(format!("GPU {i} VRAM"), format_bytes(v))),
            None => r
                .measurements
                .push(Measurement::unavailable(format!("GPU {i} VRAM"), "bytes")),
        }
        let has_temp = inv.sensors.iter().any(|s| {
            s.kind == SensorKind::Temperature
                && (s.chip == format!("GPU {i}") || s.chip.contains("amdgpu"))
        });
        if g.telemetry_source.is_none() || !has_temp {
            r.faults.push(
                FaultRecord::new(
                    "BE-GPU-020",
                    "GPU telemetry incomplete",
                    format!(
                        "GPU {i} ({}): {}",
                        g.id,
                        if g.telemetry_source.is_none() {
                            "no supported telemetry interface"
                        } else {
                            "no temperature sensor"
                        }
                    ),
                )
                .on(g.id.clone()),
            );
            status = TestStatus::Warning;
        }
    }
    if let Some(n) = expected_count
        && inv.gpus.len() as u64 != n
    {
        r.faults.push(FaultRecord::new(
            "BE-CFG-001",
            "GPU count mismatch",
            format!("{} detected, {n} expected", inv.gpus.len()),
        ));
        status = TestStatus::Fail;
    }
    r.coverage = "PCI display-class enumeration, Vulkan device enumeration, NVML/amdgpu telemetry where available".into();
    status
}

impl DiagnosticTest for GpuInventory {
    fn descriptor(&self) -> TestDescriptor {
        desc(
            "BE-T-GPU-001",
            "1.1.0",
            "GPU Inventory & Telemetry",
            Subsystem::Gpu,
            LoadClass::ReadOnly,
            Privilege::Standard,
            3,
            "Enumerates GPUs by PCI address and through Vulkan, flags software (CPU-emulated) Vulkan devices, and checks that required telemetry is exposed through NVML (NVIDIA) or amdgpu sysfs (AMD).",
            "GPU identities, Vulkan devices, VRAM size, telemetry availability",
            "Consumer and workstation cards expose different metrics. Missing telemetry blocks high-load GPU profiles.",
            json!({"expected_count": null}),
        )
    }
    fn run(&self, ctx: &TestContext) -> TestResult {
        let started = Instant::now();
        let mut r = TestResult::new(&self.descriptor(), "Graphics devices", ctx.provenance);
        let inv = ctx.platform.inventory();
        let vk = be_gpu::enumerate().map_err(|e| e.to_string());
        let st = evaluate_gpus(
            &inv,
            &vk,
            ctx.params.get("expected_count").and_then(|v| v.as_u64()),
            &mut r,
        );
        finish(r, started, st)
    }
}

// ------------------------------------------------------ device selection

/// Select devices for a GPU test. Returns (selected, excluded software devices).
pub fn select_devices(
    all: &[DeviceInfo],
    params: &serde_json::Value,
) -> (Vec<DeviceInfo>, Vec<DeviceInfo>) {
    let include_sw = params
        .get("include_software")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let wanted: Option<Vec<usize>> = params.get("devices").and_then(|v| v.as_array()).map(|a| {
        a.iter()
            .filter_map(|x| x.as_u64().map(|x| x as usize))
            .collect()
    });
    let mut sel = Vec::new();
    let mut sw = Vec::new();
    for d in all {
        if let Some(w) = &wanted
            && !w.contains(&d.index)
        {
            continue;
        }
        if d.software && !include_sw {
            sw.push(d.clone());
        } else if d.has_compute {
            sel.push(d.clone());
        }
    }
    (sel, sw)
}

fn device_label(d: &DeviceInfo) -> String {
    format!(
        "GPU{} {} [{}]{}",
        d.index,
        d.name,
        d.pci_address.clone().unwrap_or("pci unknown".into()),
        if d.software {
            " (SOFTWARE Vulkan device, not GPU hardware)"
        } else {
            ""
        }
    )
}

// ------------------------------------------------------------- telemetry

#[derive(Default, Clone)]
pub struct TelemetrySummary {
    pub samples: u64,
    pub temp_max: Option<f64>,
    pub power_max: Option<f64>,
    pub clock_min: Option<f64>,
    pub clock_max: Option<f64>,
    pub max_clock: Option<f64>,
    pub throttle: std::collections::BTreeSet<String>,
    pub replay_start: Option<u64>,
    pub replay_end: Option<u64>,
    pub source: Option<String>,
}

fn opt_max(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.max(y)),
        (x, None) => x,
        (None, y) => y,
    }
}

fn opt_min(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (x, None) => x,
        (None, y) => y,
    }
}

impl TelemetrySummary {
    pub fn add(&mut self, s: &GpuStatus) {
        self.samples += 1;
        self.source = Some(s.source.clone());
        self.temp_max = opt_max(self.temp_max, s.temperature_c);
        self.power_max = opt_max(self.power_max, s.power_w);
        self.clock_min = opt_min(self.clock_min, s.sm_clock_mhz);
        self.clock_max = opt_max(self.clock_max, s.sm_clock_mhz);
        self.max_clock = opt_max(self.max_clock, s.max_sm_clock_mhz);
        for t in &s.throttle_reasons {
            self.throttle.insert(t.clone());
        }
        if self.replay_start.is_none() {
            self.replay_start = s.pcie_replay_count;
        }
        if s.pcie_replay_count.is_some() {
            self.replay_end = s.pcie_replay_count;
        }
    }

    /// Append measurements and faults for one device.
    pub fn report(&self, label: &str, dev: &str, r: &mut TestResult) -> TestStatus {
        let mut st = TestStatus::Pass;
        if self.samples == 0 || self.source.is_none() {
            r.measurements.push(Measurement::unavailable(
                format!("{label} temperature max"),
                "°C",
            ));
            r.notes.push(format!("{label}: no vendor telemetry (NVML/amdgpu) available; thermal, power and throttling not observed"));
            return st;
        }
        let m = |r: &mut TestResult, n: &str, v: Option<f64>, u: &str| match v {
            Some(v) => r
                .measurements
                .push(Measurement::num(format!("{label} {n}"), v, u)),
            None => r
                .measurements
                .push(Measurement::unavailable(format!("{label} {n}"), u)),
        };
        m(r, "temperature max", self.temp_max, "°C");
        m(r, "board power max", self.power_max, "W");
        m(r, "SM clock min", self.clock_min, "MHz");
        m(r, "SM clock max", self.clock_max, "MHz");
        m(r, "SM clock limit", self.max_clock, "MHz");
        if !self.throttle.is_empty() {
            let list: Vec<_> = self.throttle.iter().cloned().collect();
            r.faults.push(
                FaultRecord::new(
                    "BE-GPU-021",
                    "GPU clock throttling observed",
                    format!("{label}: {}", list.join(", ")),
                )
                .on(dev),
            );
            st = TestStatus::Warning;
        }
        if let (Some(a), Some(b)) = (self.replay_start, self.replay_end) {
            r.measurements.push(Measurement::num(
                format!("{label} PCIe replay count increase"),
                b.saturating_sub(a) as f64,
                "",
            ));
            if b > a {
                r.faults.push(
                    FaultRecord::new(
                        "BE-PCIE-004",
                        "PCIe replay counter increased during test",
                        format!("{label}: {a} -> {b}"),
                    )
                    .on(dev),
                );
                st = TestStatus::Warning;
            }
        }
        st
    }
}

/// Sample vendor telemetry for `pci` every 500 ms until `stop` is set.
fn spawn_sampler(
    pci: Option<String>,
    stop: Arc<AtomicBool>,
) -> std::thread::JoinHandle<TelemetrySummary> {
    std::thread::spawn(move || {
        let mut t = TelemetrySummary::default();
        let Some(pci) = pci else { return t };
        loop {
            if let Some(s) = be_platform::gpu::gpu_status(&pci) {
                t.add(&s);
            }
            if stop.load(Ordering::Relaxed) {
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        t
    })
}

// ----------------------------------------------------------- common runner

struct DeviceRun {
    label: String,
    dev_id: String,
    status: TestStatus,
    measurements: Vec<Measurement>,
    faults: Vec<FaultRecord>,
    notes: Vec<String>,
    telemetry: TelemetrySummary,
}

type DeviceOutcome = (TestStatus, Vec<Measurement>, Vec<FaultRecord>, Vec<String>);
type DeviceWork<'a> = dyn Fn(&DeviceInfo, &TestContext) -> DeviceOutcome + Sync + 'a;

fn run_on_devices(d: TestDescriptor, ctx: &TestContext, work: &DeviceWork) -> TestResult {
    let started = Instant::now();
    let mut r = TestResult::new(&d, "Selected GPUs", ctx.provenance);
    let all = match be_gpu::enumerate() {
        Ok(a) => a,
        Err(e) => {
            r.faults.push(FaultRecord::new(
                "BE-GPU-001",
                "GPU backend unavailable",
                e.to_string(),
            ));
            r.notes.push(
                "Install the GPU vendor driver with Vulkan support (Vulkan loader and ICD).".into(),
            );
            return finish(r, started, TestStatus::Unsupported);
        }
    };
    let (sel, sw) = select_devices(&all, ctx.params);
    if sel.is_empty() {
        if !sw.is_empty() {
            r.faults.push(FaultRecord::new(
                "BE-GPU-002",
                "Only software Vulkan devices present",
                sw.iter().map(device_label).collect::<Vec<_>>().join("; "),
            ));
        } else {
            r.faults.push(FaultRecord::new(
                "BE-GPU-001",
                "No Vulkan GPU with compute support",
                "no eligible device",
            ));
        }
        return finish(r, started, TestStatus::Unsupported);
    }
    let simultaneous = ctx.p_str("mode") == Some("simultaneous");
    let run_one = |dev: &DeviceInfo| -> DeviceRun {
        let stop = Arc::new(AtomicBool::new(false));
        let sampler = spawn_sampler(dev.pci_address.clone(), stop.clone());
        let (status, measurements, faults, notes) = work(dev, ctx);
        stop.store(true, Ordering::Relaxed);
        let telemetry = sampler.join().unwrap_or_default();
        DeviceRun {
            label: device_label(dev),
            dev_id: dev
                .pci_address
                .clone()
                .unwrap_or(format!("vk{}", dev.index)),
            status,
            measurements,
            faults,
            notes,
            telemetry,
        }
    };
    let runs: Vec<DeviceRun> = if simultaneous && sel.len() > 1 {
        let out = Mutex::new(Vec::new());
        std::thread::scope(|s| {
            for dev in &sel {
                let out = &out;
                let run_one = &run_one;
                s.spawn(move || {
                    let dr = run_one(dev);
                    out.lock().unwrap().push(dr);
                });
            }
        });
        let mut v = out.into_inner().unwrap();
        v.sort_by(|a, b| a.label.cmp(&b.label));
        v
    } else {
        sel.iter().map(&run_one).collect()
    };
    let mut worst = TestStatus::Pass;
    for dr in runs {
        let mut st = dr.status;
        let ts = dr.telemetry.report(&dr.label, &dr.dev_id, &mut r);
        if ts.severity() > st.severity() {
            st = ts;
        }
        r.measurements.push(Measurement::text(
            format!("{} result", dr.label),
            st.label(),
        ));
        r.measurements.extend(dr.measurements);
        let dev_id = dr.dev_id.clone();
        r.faults.extend(dr.faults.into_iter().map(|f| {
            if f.device.is_none() {
                f.on(dev_id.clone())
            } else {
                f
            }
        }));
        r.notes.extend(dr.notes);
        if st.severity() > worst.severity() {
            worst = st;
        }
    }
    r.coverage = format!(
        "{} device(s) tested {}{}",
        sel.len(),
        if simultaneous && sel.len() > 1 {
            "simultaneously"
        } else {
            "independently"
        },
        if sel.iter().any(|d| d.software) {
            "; INCLUDES SOFTWARE VULKAN DEVICE (not GPU hardware)"
        } else {
            ""
        }
    );
    if !sw.is_empty() {
        r.notes.push(format!(
            "Excluded software devices: {}",
            sw.iter().map(device_label).collect::<Vec<_>>().join("; ")
        ));
    }
    finish(r, started, worst)
}

fn backend_error(
    label: &str,
    error: &Option<String>,
    device_lost: bool,
    f: &mut Vec<FaultRecord>,
) -> Option<TestStatus> {
    let e = error.as_ref()?;
    if device_lost {
        f.push(FaultRecord::new(
            "BE-GPU-011",
            "GPU device lost during test",
            format!("{label}: {e}"),
        ));
        Some(TestStatus::Fail)
    } else if e.contains("out of device") {
        f.push(FaultRecord::new(
            "BE-GPU-012",
            "GPU memory allocation failed",
            format!("{label}: {e}"),
        ));
        Some(TestStatus::Inconclusive)
    } else if e.contains("did not complete") {
        f.push(FaultRecord::new(
            "BE-GPU-011",
            "GPU work timed out (possible hang)",
            format!("{label}: {e}"),
        ));
        Some(TestStatus::Fail)
    } else {
        f.push(FaultRecord::new(
            "BE-GPU-001",
            "GPU backend error",
            format!("{label}: {e}"),
        ));
        Some(TestStatus::Inconclusive)
    }
}

// --------------------------------------------------------------- compute

pub struct GpuCompute;

impl DiagnosticTest for GpuCompute {
    fn descriptor(&self) -> TestDescriptor {
        desc(
            "BE-T-GPU-002",
            "1.0.0",
            "GPU Compute Verification",
            Subsystem::Gpu,
            LoadClass::Low,
            Privilege::Standard,
            10,
            "Runs a deterministic integer and exact floating-point FMA compute kernel on each GPU (Vulkan) and compares every element with a CPU reference.",
            "Elements verified, mismatches with index/expected/actual, device-lost events, telemetry",
            "Short kernel: detects deterministic compute errors, not marginal instability under long heat soak. Requires a Vulkan driver.",
            json!({"elements": 1048576, "iterations": 256, "rounds": 4, "mode": "independent", "devices": null, "include_software": false}),
        )
    }
    fn run(&self, ctx: &TestContext) -> TestResult {
        let elements = ctx.p_u64("elements", 1 << 20).clamp(64, 1 << 24) as u32;
        let iterations = ctx.p_u64("iterations", 256).clamp(1, 4096) as u32;
        let rounds = ctx.p_u64("rounds", 4).clamp(1, 1000) as u32;
        run_on_devices(self.descriptor(), ctx, &|dev, ctx| {
            let label = device_label(dev);
            let o = be_gpu::compute_kat(dev.index, elements, iterations, rounds, ctx.cancel);
            let mut f = Vec::new();
            let m = vec![
                Measurement::num(format!("{label} elements verified"), o.elements as f64, ""),
                Measurement::num(format!("{label} mismatches"), o.mismatches as f64, ""),
                Measurement::num(format!("{label} elapsed"), o.elapsed.as_secs_f64(), "s"),
            ];
            if let Some(st) = backend_error(&label, &o.error, o.device_lost, &mut f) {
                return (st, m, f, vec![]);
            }
            for (i, w, g) in &o.first {
                f.push(FaultRecord::new(
                    "BE-GPU-010",
                    "GPU compute result mismatch",
                    format!("{label}: element {i} expected {w:#010x}, got {g:#010x}"),
                ));
            }
            let st = if o.mismatches > 0 {
                TestStatus::Fail
            } else if o.cancelled || o.dispatches < rounds as u64 {
                TestStatus::Aborted
            } else {
                TestStatus::Pass
            };
            (st, m, f, vec![])
        })
    }
}

// ------------------------------------------------------------------ VRAM

pub struct GpuVram;

impl DiagnosticTest for GpuVram {
    fn descriptor(&self) -> TestDescriptor {
        desc(
            "BE-T-GPU-003",
            "1.0.0",
            "GPU VRAM Pattern Test",
            Subsystem::Vram,
            LoadClass::Moderate,
            Privilege::Standard,
            60,
            "Writes and verifies six patterns (own-address, walking ones, checkerboard, pseudo-random, zeros, ones) with moving inversions in device-local allocations, entirely on the GPU, recording the first mismatching offsets.",
            "Tested MiB, words verified, mismatches with offset/expected/actual and flipped bits, telemetry",
            "Covers only memory this process can allocate (capped at 50% of the device-local heap); driver-reserved VRAM and other processes' memory are not covered. ECC counters are not yet read.",
            json!({"size_mib": 1024, "passes": 1, "mode": "independent", "devices": null, "include_software": false}),
        )
    }
    fn run(&self, ctx: &TestContext) -> TestResult {
        let mib = ctx.p_u64("size_mib", 1024).clamp(1, 1 << 20);
        let passes = ctx.p_u64("passes", 1).clamp(1, 100) as u32;
        run_on_devices(self.descriptor(), ctx, &|dev, ctx| {
            let label = device_label(dev);
            let o = be_gpu::vram_patterns(dev.index, mib, passes, ctx.cancel, None);
            let mut f = Vec::new();
            let mut notes = Vec::new();
            let m = vec![
                Measurement::num(format!("{label} VRAM tested"), o.tested_mib as f64, "MiB"),
                Measurement::num(
                    format!("{label} VRAM requested"),
                    o.requested_mib as f64,
                    "MiB",
                ),
                Measurement::num(
                    format!("{label} patterns completed"),
                    o.patterns_completed as f64,
                    "",
                ),
                Measurement::num(
                    format!("{label} words verified"),
                    o.words_verified as f64,
                    "",
                ),
                Measurement::num(format!("{label} mismatched words"), o.mismatches as f64, ""),
                Measurement::num(format!("{label} elapsed"), o.elapsed.as_secs_f64(), "s"),
            ];
            if let Some(st) = backend_error(&label, &o.error, o.device_lost, &mut f) {
                return (st, m, f, notes);
            }
            for (off, w, g) in &o.first {
                f.push(FaultRecord::new(
                    "BE-GPU-014",
                    "GPU memory mismatch",
                    format!("{label}: word offset {off:#x}: expected {w:#010x}, read {g:#010x} ({} bit(s) differ)", (w ^ g).count_ones()),
                ));
            }
            if o.allocation_limited {
                f.push(FaultRecord::new(
                    "BE-GPU-012",
                    "VRAM coverage reduced",
                    format!(
                        "{label}: tested {} of {} MiB requested",
                        o.tested_mib, o.requested_mib
                    ),
                ));
                notes.push(format!(
                    "{label}: allocation limited by 50% heap cap or driver"
                ));
            }
            let total = passes as u64 * be_gpu::VRAM_PATTERNS.len() as u64;
            let st = if o.mismatches > 0 {
                TestStatus::Fail
            } else if o.cancelled || o.patterns_completed < total {
                TestStatus::Aborted
            } else if o.allocation_limited {
                TestStatus::Warning
            } else {
                TestStatus::Pass
            };
            (st, m, f, notes)
        })
    }
}

// -------------------------------------------------------------- graphics

pub struct GpuGraphics;

impl DiagnosticTest for GpuGraphics {
    fn descriptor(&self) -> TestDescriptor {
        let mut d = desc(
            "BE-T-GPU-004",
            "1.0.0",
            "GPU Graphics Stress (FurMark-inspired)",
            Subsystem::Gpu,
            LoadClass::High,
            Privilege::Standard,
            600,
            "Sustained offscreen fragment-shading load with an original procedural shader. Reference frames are re-rendered periodically and compared bit-exactly with the first; telemetry records temperature, power, clocks and throttle reasons.",
            "Frames rendered, verification frames, inconsistent frames, device-lost events, telemetry",
            "Original workload; not FurMark and not comparable to its scores. Offscreen only (no display output tested). Requires authorization and an approved safety profile.",
            json!({"duration_s": 600, "width": 1920, "height": 1080, "layers": 48, "frames_per_submit": 8, "verify_every": 20, "mode": "independent", "devices": null, "include_software": false}),
        );
        d.cancel_behavior =
            "Checked between submissions (< 1 s); supervisor kills the worker on timeout".into();
        d
    }
    fn run(&self, ctx: &TestContext) -> TestResult {
        let p = be_gpu::GraphicsParams {
            duration: Duration::from_secs(ctx.p_u64("duration_s", 600).max(1)),
            width: ctx.p_u64("width", 1920).clamp(64, 7680) as u32,
            height: ctx.p_u64("height", 1080).clamp(64, 4320) as u32,
            layers: ctx.p_u64("layers", 48).clamp(1, 512) as u32,
            frames_per_submit: ctx.p_u64("frames_per_submit", 8).clamp(1, 64) as u32,
            verify_every: ctx.p_u64("verify_every", 20).clamp(1, 10_000) as u32,
        };
        run_on_devices(self.descriptor(), ctx, &|dev, ctx| {
            let label = device_label(dev);
            let o = be_gpu::graphics_load(dev.index, p, ctx.cancel, &|f| {
                (ctx.progress)(Some(f), "sustained graphics load")
            });
            let mut f = Vec::new();
            let m = vec![
                Measurement::num(format!("{label} frames rendered"), o.frames as f64, ""),
                Measurement::derived(
                    format!("{label} frame rate"),
                    o.frames as f64 / o.elapsed.as_secs_f64().max(1e-6),
                    "frames/s",
                ),
                Measurement::num(
                    format!("{label} verification frames"),
                    o.verification_frames as f64,
                    "",
                ),
                Measurement::num(
                    format!("{label} inconsistent frames"),
                    o.inconsistent_frames as f64,
                    "",
                ),
                Measurement::text(
                    format!("{label} resolution"),
                    format!("{}x{}, {} layers", o.resolution.0, o.resolution.1, o.layers),
                ),
            ];
            if let Some(st) = backend_error(&label, &o.error, o.device_lost, &mut f) {
                return (st, m, f, vec![]);
            }
            if o.inconsistent_frames > 0 {
                f.push(FaultRecord::new(
                    "BE-GPU-030",
                    "Rendering inconsistency",
                    format!(
                        "{label}: {} of {} reference frames differed",
                        o.inconsistent_frames, o.verification_frames
                    ),
                ));
            }
            let st = if o.inconsistent_frames > 0 {
                TestStatus::Fail
            } else if o.cancelled || o.elapsed + Duration::from_millis(500) < p.duration {
                TestStatus::Aborted
            } else {
                TestStatus::Pass
            };
            (st, m, f, vec![])
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use be_core::inventory::GpuDevice;

    fn dev(i: usize, sw: bool) -> DeviceInfo {
        DeviceInfo {
            index: i,
            name: format!("d{i}"),
            vendor_id: 0,
            vendor: "x".into(),
            device_id: 0,
            device_type: if sw {
                "CPU".into()
            } else {
                "DISCRETE_GPU".into()
            },
            software: sw,
            api_version: "1.3.0".into(),
            driver_version: 0,
            pci_address: None,
            device_local_bytes: 1 << 30,
            max_storage_buffer_range: u32::MAX,
            max_alloc_bytes: None,
            max_workgroup_count_x: 65535,
            has_compute: true,
            has_graphics: true,
        }
    }

    #[test]
    fn software_devices_excluded_by_default() {
        let all = vec![dev(0, true), dev(1, false), dev(2, false)];
        let (s, sw) = select_devices(&all, &json!({}));
        assert_eq!(s.iter().map(|d| d.index).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(sw.len(), 1);
        let (s, _) = select_devices(&all, &json!({"include_software": true, "devices": [0, 2]}));
        assert_eq!(s.iter().map(|d| d.index).collect::<Vec<_>>(), vec![0, 2]);
    }

    #[test]
    fn telemetry_detects_throttle_and_replay() {
        let mut t = TelemetrySummary::default();
        t.add(&GpuStatus {
            source: "NVML".into(),
            temperature_c: Some(60.0),
            sm_clock_mhz: Some(2400.0),
            pcie_replay_count: Some(5),
            ..Default::default()
        });
        t.add(&GpuStatus {
            source: "NVML".into(),
            temperature_c: Some(83.0),
            sm_clock_mhz: Some(1800.0),
            pcie_replay_count: Some(9),
            throttle_reasons: vec!["hardware thermal slowdown".into()],
            ..Default::default()
        });
        let mut r =
            TestResult::not_executed("X", "1", TestStatus::Running, "", Provenance::Fixture);
        assert_eq!(
            t.report("GPU0", "0000:01:00.0", &mut r),
            TestStatus::Warning
        );
        let codes: Vec<_> = r.faults.iter().map(|f| f.code.as_str()).collect();
        assert!(codes.contains(&"BE-GPU-021") && codes.contains(&"BE-PCIE-004"));
        assert!(
            r.measurements
                .iter()
                .any(|m| m.name.ends_with("temperature max") && m.value == Some(83.0))
        );
        let mut r2 =
            TestResult::not_executed("X", "1", TestStatus::Running, "", Provenance::Fixture);
        assert_eq!(
            TelemetrySummary::default().report("GPU0", "x", &mut r2),
            TestStatus::Pass
        );
        assert!(r2.measurements[0].quality == DataQuality::Unavailable);
    }

    #[test]
    fn inventory_evaluation() {
        let mut r =
            TestResult::not_executed("X", "1", TestStatus::Running, "", Provenance::Fixture);
        assert_eq!(
            evaluate_gpus(&Inventory::default(), &Err("none".into()), None, &mut r),
            TestStatus::Unsupported
        );
        let mut r =
            TestResult::not_executed("X", "1", TestStatus::Running, "", Provenance::Fixture);
        assert_eq!(
            evaluate_gpus(&Inventory::default(), &Ok(vec![]), Some(2), &mut r),
            TestStatus::Fail
        );
        let inv = Inventory {
            gpus: vec![GpuDevice {
                id: "0000:01:00.0".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut r =
            TestResult::not_executed("X", "1", TestStatus::Running, "", Provenance::Fixture);
        assert_eq!(
            evaluate_gpus(&inv, &Ok(vec![dev(0, true)]), None, &mut r),
            TestStatus::Warning
        );
        assert!(
            r.measurements
                .iter()
                .any(|m| m.text.as_deref().unwrap_or("").contains("SOFTWARE"))
        );
    }
}
