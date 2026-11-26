//! Application services shared by the GUI and the CLI: profile loading,
//! the profile runner (scheduler), release review and capability registry.
//! GUI and CLI call exactly the same functions so behaviour cannot diverge.

use be_core::capability::{Capability, CapabilityState};
use be_core::model::{
    EventLevel, EventRecord, FaultRecord, LoadClass, Provenance, TestDescriptor, TestResult,
};
use be_core::policy::{self, ReleaseEvaluation, ReleaseState};
use be_core::profile::{Profile, ProfileError};
use be_core::status::TestStatus;
use be_core::time::UtcMillis;
use be_platform::PlatformProvider;
use be_safety::policy::{Authorization, preflight};
use be_safety::supervisor::default_worker_path;
use be_safety::{SensorWatchdog, SupervisorConfig, SupervisorEvent, run_supervised};
use be_storage::{Disposition, NewRun, Store};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub const BUILTIN_PROFILES: &[(&str, &str)] = &[
    ("quick", include_str!("../../../profiles/quick.json")),
    ("standard", include_str!("../../../profiles/standard.json")),
    (
        "extended-stability",
        include_str!("../../../profiles/extended-stability.json"),
    ),
    (
        "factory-burn-in",
        include_str!("../../../profiles/factory-burn-in.json"),
    ),
];

pub fn known_test(fixture_mode: bool) -> impl Fn(&str) -> bool {
    let ids: Vec<String> = be_diagnostics::descriptors(fixture_mode)
        .into_iter()
        .map(|d| d.id)
        .collect();
    move |id: &str| ids.iter().any(|x| x == id)
}

pub fn builtin_profiles() -> Vec<Profile> {
    BUILTIN_PROFILES
        .iter()
        .filter_map(|(_, j)| Profile::from_json(j).ok())
        .collect()
}

/// Load a profile by built-in id or from a JSON file path, and validate it.
pub fn load_profile(name_or_path: &str, fixture_mode: bool) -> Result<Profile, String> {
    let text = match BUILTIN_PROFILES.iter().find(|(id, _)| *id == name_or_path) {
        Some((_, j)) => j.to_string(),
        None => be_core::fsutil::read_limited(
            std::path::Path::new(name_or_path),
            be_core::fsutil::MAX_CONFIG_BYTES,
        )
        .map_err(|e| format!("cannot read profile `{name_or_path}`: {e}"))?,
    };
    let p = Profile::from_json(&text).map_err(|e| e.to_string())?;
    p.validate(&known_test(fixture_mode))
        .map_err(|e: ProfileError| e.to_string())?;
    Ok(p)
}

#[derive(Debug, Clone)]
pub struct StepInfo {
    pub key: String,
    pub test_id: String,
    pub name: String,
    pub required: bool,
    pub load: Option<LoadClass>,
    pub device: String,
}

#[derive(Debug, Clone)]
pub enum EngineEvent {
    RunStarted {
        run_id: Option<i64>,
        profile: String,
        steps: Vec<StepInfo>,
    },
    StepStarted {
        index: usize,
    },
    StepProgress {
        index: usize,
        progress: Option<f64>,
        phase: String,
    },
    StepFinished {
        index: usize,
        result: Box<TestResult>,
    },
    Log(EventRecord),
    RunFinished {
        run_id: Option<i64>,
        overall: TestStatus,
        evaluation: Option<ReleaseEvaluation>,
    },
}

#[derive(Clone)]
pub struct EngineConfig {
    pub worker_path: PathBuf,
    pub fixture_mode: bool,
    pub auth: Authorization,
    pub station: String,
    pub operator: String,
}

impl EngineConfig {
    pub fn new(operator: &str) -> Self {
        EngineConfig {
            worker_path: default_worker_path(),
            fixture_mode: false,
            auth: Authorization {
                operator: operator.into(),
                ..Default::default()
            },
            station: hostname(),
            operator: operator.into(),
        }
    }
}

pub fn hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .unwrap_or_else(|| "local-station".into())
}

#[derive(Debug, Clone)]
pub struct RunSummary {
    pub run_id: Option<i64>,
    pub overall: TestStatus,
    pub results: Vec<(String, TestResult)>,
    pub evaluation: Option<ReleaseEvaluation>,
}

fn worst(results: &[(String, TestResult)]) -> TestStatus {
    results
        .iter()
        .map(|(_, r)| r.status)
        .max_by_key(|s| s.severity())
        .unwrap_or(TestStatus::NotRun)
}

/// Run a profile end-to-end. Persists to `store` when provided.
#[allow(clippy::too_many_arguments)]
pub fn run_profile(
    cfg: &EngineConfig,
    profile: &Profile,
    unit_serial: Option<&str>,
    parent_run: Option<i64>,
    platform: &dyn PlatformProvider,
    store: Option<&Store>,
    cancel: &AtomicBool,
    on_event: &mut dyn FnMut(EngineEvent),
) -> Result<RunSummary, String> {
    let descs: HashMap<String, TestDescriptor> = be_diagnostics::descriptors(cfg.fixture_mode)
        .into_iter()
        .map(|d| (d.id.clone(), d))
        .collect();
    profile
        .validate(&|id| descs.contains_key(id))
        .map_err(|e| e.to_string())?;
    if let Some(s) = unit_serial
        && let Some(st) = store
        && st.get_unit(s).map_err(|e| e.to_string())?.is_none()
    {
        return Err(format!(
            "unit `{s}` is not registered; create the unit record first"
        ));
    }

    let emit_log = |store: Option<&Store>,
                    run_id: Option<i64>,
                    ev: EventRecord,
                    on_event: &mut dyn FnMut(EngineEvent)| {
        if let Some(st) = store {
            let _ = st.add_event(run_id, &ev);
        }
        on_event(EngineEvent::Log(ev));
    };

    let inventory = platform.inventory();
    let inventory_json = serde_json::to_string(&inventory).ok();
    let run_id = match store {
        Some(st) => Some(
            st.begin_run(&NewRun {
                unit_serial: unit_serial.map(String::from),
                parent_run_id: parent_run,
                profile_id: profile.id.clone(),
                profile_version: profile.version.clone(),
                profile_json: serde_json::to_string(profile).unwrap_or_default(),
                station: cfg.station.clone(),
                operator: cfg.operator.clone(),
                inventory_json: inventory_json.clone(),
                high_load_authorized: cfg.auth.high_load,
                safety_profile_id: cfg
                    .auth
                    .safety_profile
                    .as_ref()
                    .map(|p| format!("{}@{}", p.id, p.version)),
                fixture: cfg.fixture_mode,
            })
            .map_err(|e| e.to_string())?,
        ),
        None => None,
    };
    if let (Some(st), Some(serial)) = (store, unit_serial) {
        let _ = st.record_disposition(&Disposition {
            unit_serial: serial.into(),
            run_id,
            state: ReleaseState::Testing,
            actor: cfg.operator.clone(),
            reason: format!("profile {}@{} started", profile.id, profile.version),
            ts: UtcMillis::now(),
        });
    }

    let steps: Vec<StepInfo> = profile
        .steps
        .iter()
        .map(|s| {
            let d = descs.get(&s.test);
            StepInfo {
                key: s.key().to_string(),
                test_id: s.test.clone(),
                name: d.map(|d| d.name.clone()).unwrap_or(s.test.clone()),
                required: s.required,
                load: d.map(|d| d.load),
                device: d
                    .map(|d| d.subsystem.label().to_string())
                    .unwrap_or_default(),
            }
        })
        .collect();
    on_event(EngineEvent::RunStarted {
        run_id,
        profile: format!("{} {}", profile.name, profile.version),
        steps,
    });
    emit_log(
        store,
        run_id,
        EventRecord::new(
            EventLevel::Info,
            format!(
                "Run {} started: profile {}@{} on {} ({}){}",
                run_id.map(|i| i.to_string()).unwrap_or("(unsaved)".into()),
                profile.id,
                profile.version,
                cfg.station,
                platform.name(),
                if cfg.fixture_mode {
                    " — FIXTURE MODE, results not valid for release"
                } else {
                    ""
                }
            ),
        ),
        on_event,
    );

    let mut results: Vec<(String, TestResult)> = Vec::new();
    for (index, step) in profile.steps.iter().enumerate() {
        let key = step.key().to_string();
        let d = &descs[&step.test];
        let prov = if cfg.fixture_mode {
            Provenance::Fixture
        } else {
            Provenance::Live
        };
        let result = if cancel.load(Ordering::SeqCst) {
            TestResult::not_executed(
                &d.id,
                &d.version,
                TestStatus::NotRun,
                "Run stopped by operator before this step",
                prov,
            )
        } else if let Some(missing) = step.requires.iter().find(|r| {
            !results
                .iter()
                .any(|(k, res)| k == *r && res.status.satisfies_required(profile.allow_warnings))
        }) {
            let mut r = TestResult::not_executed(
                &d.id,
                &d.version,
                TestStatus::NotRun,
                format!("Prerequisite `{missing}` did not pass"),
                prov,
            );
            r.faults.push(FaultRecord::new(
                "BE-SYS-004",
                "Prerequisite failed",
                format!("`{missing}` did not pass"),
            ));
            emit_log(
                store,
                run_id,
                EventRecord::new(
                    EventLevel::Warn,
                    format!(
                        "{}: not started, prerequisite `{missing}` did not pass",
                        d.name
                    ),
                )
                .with_code("BE-SYS-004"),
                on_event,
            );
            r
        } else {
            let sensors = platform.sample_sensors();
            match preflight(d, &cfg.auth, &sensors) {
                Err(reason) => {
                    let mut r = TestResult::not_executed(
                        &d.id,
                        &d.version,
                        TestStatus::NotRun,
                        reason.clone(),
                        prov,
                    );
                    r.faults.push(FaultRecord::new(
                        "BE-SAF-001",
                        "Safety preflight refused the test",
                        reason.clone(),
                    ));
                    emit_log(
                        store,
                        run_id,
                        EventRecord::new(EventLevel::Warn, format!("{}: {reason}", d.name))
                            .with_code("BE-SAF-001"),
                        on_event,
                    );
                    r
                }
                Ok(()) => {
                    on_event(EngineEvent::StepStarted { index });
                    if d.load == LoadClass::High {
                        emit_log(
                            store,
                            run_id,
                            EventRecord::new(
                                EventLevel::Warn,
                                format!(
                                    "HIGH LOAD: starting {} (authorized by {})",
                                    d.name, cfg.auth.operator
                                ),
                            ),
                            on_event,
                        );
                    } else {
                        emit_log(
                            store,
                            run_id,
                            EventRecord::new(EventLevel::Info, format!("Starting {}", d.name)),
                            on_event,
                        );
                    }
                    let mut sc = SupervisorConfig::new(
                        cfg.worker_path.clone(),
                        Duration::from_secs(step.timeout_s),
                    );
                    sc.fixture_mode = cfg.fixture_mode;
                    sc.heartbeat_timeout = Duration::from_secs(30);
                    sc.high_load_authorized = d.load == LoadClass::High && cfg.auth.high_load;
                    let mut watchdog = if d.load == LoadClass::High {
                        cfg.auth.safety_profile.clone().map(|p| {
                            let provider = be_platform::native();
                            SensorWatchdog::new(p, Box::new(move || provider.sample_sensors()))
                        })
                    } else {
                        None
                    };
                    let mut pending_logs = Vec::new();
                    let r = run_supervised(
                        &sc,
                        &d.id,
                        &step.params,
                        cancel,
                        watchdog
                            .as_mut()
                            .map(|w| w as &mut dyn be_safety::SafetyMonitor),
                        &mut |e| match e {
                            SupervisorEvent::Progress { progress, phase } => {
                                on_event(EngineEvent::StepProgress {
                                    index,
                                    progress,
                                    phase,
                                })
                            }
                            SupervisorEvent::Log {
                                level,
                                code,
                                message,
                            } => {
                                let mut ev =
                                    EventRecord::new(level, format!("{}: {message}", d.name));
                                ev.code = code;
                                pending_logs.push(ev.clone());
                                on_event(EngineEvent::Log(ev));
                            }
                            SupervisorEvent::Started { .. } => {}
                        },
                    );
                    if let Some(st) = store {
                        for ev in pending_logs {
                            let _ = st.add_event(run_id, &ev);
                        }
                    }
                    r
                }
            }
        };
        let level = match result.status {
            TestStatus::Pass => EventLevel::Pass,
            TestStatus::Fail | TestStatus::Aborted => EventLevel::Error,
            TestStatus::Warning | TestStatus::Inconclusive => EventLevel::Warn,
            _ => EventLevel::Info,
        };
        let mut msg = format!("{} [{}]: {}", d.name, key, result.status.label());
        if let Some(f) = result.faults.first() {
            msg.push_str(&format!(" — {}: {}", f.code, f.message));
        } else if let Some(n) = result.notes.first() {
            msg.push_str(&format!(" — {n}"));
        }
        let mut ev = EventRecord::new(level, msg);
        ev.code = result.first_fault_code().map(String::from);
        emit_log(store, run_id, ev, on_event);
        if let (Some(st), Some(id)) = (store, run_id)
            && let Err(e) = st.insert_result(id, &key, &result)
        {
            emit_log(
                store,
                run_id,
                EventRecord::new(
                    EventLevel::Error,
                    format!("Could not persist result for {key}: {e}"),
                ),
                on_event,
            );
        }
        on_event(EngineEvent::StepFinished {
            index,
            result: Box::new(result.clone()),
        });
        results.push((key, result));
    }

    let overall = worst(&results);
    let stopped = cancel.load(Ordering::SeqCst);
    let mut evaluation = None;
    if let (Some(st), Some(id)) = (store, run_id) {
        let _ = st.finish_run(
            id,
            if stopped { "aborted" } else { "completed" },
            overall.label(),
        );
        if let Some(serial) = unit_serial {
            let manual = st.manual_checks(serial, Some(id)).unwrap_or_default();
            let waivers = st.waivers(serial, id).unwrap_or_default();
            let map: HashMap<String, TestResult> = results.iter().cloned().collect();
            let e = policy::evaluate(profile, &map, &manual, &waivers);
            let state = e.recommended_state;
            let _ = st.record_disposition(&Disposition {
                unit_serial: serial.into(),
                run_id: Some(id),
                state,
                actor: "policy-engine".into(),
                reason: if e.blockers.is_empty() {
                    "all required tests passed; awaiting authorized review".into()
                } else {
                    e.blockers.join("; ")
                },
                ts: UtcMillis::now(),
            });
            evaluation = Some(e);
        }
    }
    emit_log(
        store,
        run_id,
        EventRecord::new(
            if overall == TestStatus::Pass {
                EventLevel::Pass
            } else if overall == TestStatus::Fail {
                EventLevel::Error
            } else {
                EventLevel::Warn
            },
            format!(
                "Run finished: overall {}{}",
                overall.label(),
                if stopped {
                    " (stopped by operator)"
                } else {
                    ""
                }
            ),
        ),
        on_event,
    );
    on_event(EngineEvent::RunFinished {
        run_id,
        overall,
        evaluation: evaluation.clone(),
    });
    Ok(RunSummary {
        run_id,
        overall,
        results,
        evaluation,
    })
}

/// Re-evaluate the latest run of a unit and, only if eligible, record a
/// `Released` disposition by the named reviewer.
pub fn approve_release(
    store: &Store,
    serial: &str,
    reviewer: &str,
    note: &str,
) -> Result<ReleaseEvaluation, ReleaseEvaluation> {
    let refuse = |msg: String| ReleaseEvaluation {
        eligible_for_review: false,
        recommended_state: ReleaseState::Hold,
        blockers: vec![msg],
        notes: vec![],
    };
    if reviewer.trim().is_empty() {
        return Err(refuse(
            "release approval requires a reviewer identity".into(),
        ));
    }
    let Some(run) = store
        .latest_run_for_unit(serial)
        .map_err(|e| refuse(e.to_string()))?
    else {
        return Err(refuse(format!("unit {serial} has no completed run")));
    };
    if run.fixture {
        return Err(refuse(format!(
            "run {} was executed in FIXTURE mode",
            run.id
        )));
    }
    let profile: Profile = serde_json::from_str(&run.profile_json)
        .map_err(|e| refuse(format!("run profile unreadable: {e}")))?;
    let results: HashMap<String, TestResult> = store
        .results_for_run(run.id)
        .map_err(|e| refuse(e.to_string()))?
        .into_iter()
        .collect();
    let manual = store
        .manual_checks(serial, Some(run.id))
        .map_err(|e| refuse(e.to_string()))?;
    let waivers = store
        .waivers(serial, run.id)
        .map_err(|e| refuse(e.to_string()))?;
    let e = policy::evaluate(&profile, &results, &manual, &waivers);
    if !e.eligible_for_review {
        let _ = store.audit(
            reviewer,
            "release.refused",
            &format!(
                "serial={serial} run={} blockers={}",
                run.id,
                e.blockers.join("; ")
            ),
        );
        return Err(e);
    }
    store
        .record_disposition(&Disposition {
            unit_serial: serial.into(),
            run_id: Some(run.id),
            state: ReleaseState::Released,
            actor: reviewer.into(),
            reason: if note.is_empty() {
                "release approved after review".into()
            } else {
                note.into()
            },
            ts: UtcMillis::now(),
        })
        .map_err(|er| refuse(er.to_string()))?;
    Ok(e)
}

/// Evaluate (without changing state) the latest run of a unit.
pub fn evaluate_unit(store: &Store, serial: &str) -> Result<(i64, ReleaseEvaluation), String> {
    let run = store
        .latest_run_for_unit(serial)
        .map_err(|e| e.to_string())?
        .ok_or(format!("unit {serial} has no completed run"))?;
    let profile: Profile = serde_json::from_str(&run.profile_json).map_err(|e| e.to_string())?;
    let results: HashMap<String, TestResult> = store
        .results_for_run(run.id)
        .map_err(|e| e.to_string())?
        .into_iter()
        .collect();
    let manual = store
        .manual_checks(serial, Some(run.id))
        .map_err(|e| e.to_string())?;
    let waivers = store.waivers(serial, run.id).map_err(|e| e.to_string())?;
    let mut e = policy::evaluate(&profile, &results, &manual, &waivers);
    if run.fixture {
        e.eligible_for_review = false;
        e.blockers.insert(0, "run executed in FIXTURE mode".into());
    }
    Ok((run.id, e))
}

/// Capability registry for this build on this machine.
pub fn capabilities(platform: &dyn PlatformProvider) -> Vec<Capability> {
    use CapabilityState::*;
    let inv = platform.inventory();
    let mut c = Vec::new();
    let has = |item: &str| {
        inv.unavailable
            .iter()
            .find(|u| u.item == item)
            .map(|u| u.reason.clone())
    };
    c.push(Capability::new(
        "inventory",
        "Hardware inventory",
        "System Information",
        platform.name(),
        if inv.cpu.is_some() {
            Available
        } else {
            Unsupported("no CPU data".into())
        },
    ));
    c.push(Capability::new(
        "sensors.temperature",
        "Temperature telemetry",
        "Monitoring",
        "hwmon / thermal / NVML / ACPI",
        match has("Temperature sensors") {
            None => Available,
            Some(r) => Unsupported(r),
        },
    ));
    c.push(Capability::new(
        "sensors.fan",
        "Fan tachometers",
        "Monitoring",
        "hwmon",
        match has("Fan tachometers") {
            None => Available,
            Some(r) => Unsupported(r),
        },
    ));
    c.push(Capability::new(
        "sensors.power",
        "Power telemetry",
        "Power",
        "hwmon / NVML",
        match has("Power telemetry") {
            None => Available,
            Some(r) => Unsupported(r),
        },
    ));
    c.push(Capability::new(
        "memory.ecc",
        "ECC error counters",
        "Diagnostics",
        "EDAC",
        match has("ECC error counters") {
            None => Available,
            Some(r) => Unsupported(r),
        },
    ));
    c.push(Capability::new(
        "cpu.kat",
        "CPU known-answer verification",
        "Diagnostics",
        "be-worker",
        Available,
    ));
    c.push(Capability::new(
        "memory.pattern",
        "RAM pattern test (allocated buffer)",
        "Diagnostics",
        "be-worker",
        Available,
    ));
    c.push(Capability::new(
        "storage.scratch",
        "Scratch-file integrity",
        "Diagnostics",
        "be-worker",
        Available,
    ));
    let smartctl = std::process::Command::new(
        be_core::fsutil::trusted_tool("smartctl").unwrap_or_else(|| "smartctl-not-found".into()),
    )
    .arg("--version")
    .output()
    .map(|o| o.status.success())
    .unwrap_or(false);
    c.push(Capability::new(
        "storage.smart",
        "SMART / NVMe health",
        "Diagnostics",
        "smartctl (external)",
        if smartctl {
            Degraded("usually requires administrator/root".into())
        } else {
            Unsupported("smartmontools `smartctl` not installed".into())
        },
    ));
    c.push(Capability::new(
        "pcie.link",
        "PCIe link and AER",
        "Diagnostics",
        "sysfs",
        if inv.pci.iter().any(|p| p.max_link_width.is_some()) {
            Available
        } else {
            Unsupported("no PCIe link data exposed".into())
        },
    ));
    c.push(Capability::new(
        "gpu.inventory",
        "GPU inventory",
        "Graphics",
        "PCI / NVML / amdgpu",
        if inv.gpus.is_empty() {
            Unsupported("no GPUs detected".into())
        } else {
            Available
        },
    ));
    c.push(Capability::new(
        "gpu.nvml",
        "NVIDIA telemetry (NVML)",
        "Graphics",
        "NVML (runtime-loaded)",
        match be_platform::gpu::nvml_probe() {
            Ok(n) => {
                if n > 0 {
                    Available
                } else {
                    Unsupported("NVML loaded, no NVIDIA devices".into())
                }
            }
            Err(e) => Unsupported(e),
        },
    ));
    c.push(Capability::new(
        "gpu.compute",
        "GPU compute verification",
        "Stress Tests",
        "Vulkan/CUDA/HIP",
        Planned("No GPU compute backend implemented (Milestone 3)".into()),
    ));
    c.push(Capability::new(
        "gpu.vram",
        "VRAM pattern test",
        "Stress Tests",
        "Vulkan/CUDA/HIP",
        Planned("No GPU memory backend implemented (Milestone 3)".into()),
    ));
    c.push(Capability::new(
        "gpu.graphics",
        "Graphics stress (FurMark-style)",
        "Stress Tests",
        "Vulkan",
        Planned("No graphics backend implemented (Milestone 3)".into()),
    ));
    c.push(Capability::new(
        "stress.cpu",
        "CPU verified stress (Prime95-style)",
        "Stress Tests",
        "be-worker",
        Blocked("requires operator authorization and an approved hardware safety profile".into()),
    ));
    c.push(Capability::new(
        "stress.memory",
        "RAM extended integrity",
        "Stress Tests",
        "be-worker",
        Blocked("requires operator authorization and an approved hardware safety profile".into()),
    ));
    c.push(Capability::new(
        "stress.combined",
        "Combined CPU + RAM load",
        "Burn-In",
        "be-worker",
        Blocked("requires authorization; GPUs not included".into()),
    ));
    c.push(Capability::new(
        "network.link",
        "Network link review",
        "Diagnostics",
        "sysfs / CIM",
        if inv.network.iter().any(|n| !n.is_virtual) {
            Available
        } else {
            Unsupported("no physical interfaces".into())
        },
    ));
    c.push(Capability::new(
        "network.throughput",
        "Network throughput (iperf3 peer)",
        "Diagnostics",
        "iperf3",
        Planned("Requires an authorized peer; not implemented".into()),
    ));
    c.push(Capability::new(
        "firmware.security",
        "Firmware / Secure Boot / TPM review",
        "Security",
        platform.name(),
        Available,
    ));
    c.push(Capability::new(
        "display.edid",
        "Display EDID / test patterns",
        "Displays",
        "DRM / DXGI",
        Planned("Not implemented".into()),
    ));
    c.push(Capability::new(
        "reports.html",
        "HTML / JSON / CSV reports",
        "Reports",
        "be-reporting",
        Available,
    ));
    c.push(Capability::new(
        "reports.pdf",
        "Native PDF reports",
        "Reports",
        "-",
        Planned(
            "Use HTML report and print to PDF; native PDF pending library license review".into(),
        ),
    ));
    c.push(Capability::new(
        "preboot",
        "Bootable diagnostics environment",
        "Preboot",
        "-",
        Planned("Milestone 6".into()),
    ));
    c
}

/// Resolve the database path for both front ends.
pub fn open_store(path: Option<&Path>) -> Result<Store, String> {
    let p = path
        .map(PathBuf::from)
        .unwrap_or_else(be_storage::default_db_path);
    let s = Store::open(&p).map_err(|e| format!("cannot open database {}: {e}", p.display()))?;
    let _ = s.recover_interrupted();
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_profiles_validate() {
        for (id, _) in BUILTIN_PROFILES {
            load_profile(id, false).unwrap_or_else(|e| panic!("{id}: {e}"));
        }
        assert_eq!(builtin_profiles().len(), BUILTIN_PROFILES.len());
    }

    #[test]
    fn example_safety_profile_is_not_approved() {
        let p = be_safety::HardwareSafetyProfile::from_json(include_str!(
            "../../../profiles/safety/EXAMPLE-not-approved.json"
        ))
        .unwrap();
        assert!(!p.approved);
    }

    #[test]
    fn quick_profile_contains_no_high_load() {
        let p = load_profile("quick", false).unwrap();
        let d = be_diagnostics::descriptors(false);
        for s in &p.steps {
            let desc = d.iter().find(|x| x.id == s.test).unwrap();
            assert!(
                desc.load < LoadClass::Moderate,
                "{} in quick profile",
                s.test
            );
        }
    }
}
