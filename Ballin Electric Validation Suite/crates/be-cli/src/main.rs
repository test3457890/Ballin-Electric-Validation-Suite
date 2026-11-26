//! `bevalidate` — command-line interface for automated factory operation.
//! Uses the same engine, policy and storage as the GUI.
//!
//! Exit codes: 0 pass, 1 fail, 2 incomplete/unsupported, 3 aborted,
//! 4 invalid input, 5 internal error, 6 not authorized / release refused.

use anyhow::{Context, Result, anyhow};
use be_core::exit;
use be_core::inventory::{Inventory, format_bytes};
use be_core::model::EventRecord;
use be_core::policy::{ManualCheckRecord, ManualCheckStatus, ReleaseState, Waiver};
use be_core::time::UtcMillis;
use be_engine::{EngineConfig, EngineEvent};
use be_reporting::{Audience, ReportData};
use be_storage::{Store, Unit};
use clap::{Parser, Subcommand, ValueEnum};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Parser)]
#[command(name = "bevalidate", version, about = "Ballin Electric System Validation Suite — command line", long_about = None)]
struct Cli {
    /// SQLite database path (default: per-user data directory, or $BE_VALIDATION_DB)
    #[arg(long, global = true)]
    db: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Clone, Copy, ValueEnum, PartialEq)]
enum Format {
    Text,
    Json,
}

#[derive(Clone, Copy, ValueEnum, PartialEq)]
enum ReportFormat {
    Json,
    Csv,
    Html,
    Pdf,
}

#[derive(Clone, Copy, ValueEnum, PartialEq)]
enum AudienceArg {
    Internal,
    Customer,
}

#[derive(Subcommand)]
enum Cmd {
    /// Read-only hardware inventory
    Inventory {
        #[arg(long, value_enum, default_value = "text")]
        format: Format,
    },
    /// What this build can do on this machine
    Capabilities {
        #[arg(long, value_enum, default_value = "text")]
        format: Format,
    },
    /// Live sensor readings (read-only)
    Sensors {
        #[arg(long, value_enum, default_value = "text")]
        format: Format,
        /// Repeat every second for N seconds
        #[arg(long, default_value_t = 0)]
        watch: u64,
    },
    /// Test modules
    Tests {
        #[command(subcommand)]
        cmd: TestsCmd,
    },
    /// Test profiles
    Profile {
        #[command(subcommand)]
        cmd: ProfileCmd,
    },
    /// Execute a test profile
    Run(RunArgs),
    /// Generate a report for a stored run
    Report {
        #[arg(long)]
        run_id: i64,
        #[arg(long, value_enum, default_value = "html")]
        format: ReportFormat,
        #[arg(long, value_enum, default_value = "internal")]
        audience: AudienceArg,
        /// Output file (default: stdout)
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// List stored runs
    Runs {
        #[arg(long)]
        unit: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: u32,
    },
    /// Show stored events
    Events {
        #[arg(long)]
        run_id: Option<i64>,
        #[arg(long, default_value_t = 200)]
        limit: u32,
    },
    /// Manufacturing unit records
    Unit {
        #[command(subcommand)]
        cmd: UnitCmd,
    },
    /// Sign off a manual inspection check for a run
    Check {
        #[arg(long)]
        unit: String,
        #[arg(long)]
        run_id: i64,
        #[arg(long)]
        check: String,
        #[arg(long, value_parser = ["pass", "fail", "na"])]
        status: String,
        #[arg(long)]
        operator: String,
        #[arg(long, default_value = "")]
        note: String,
    },
    /// Record a reviewer waiver for a non-failed, waivable step
    Waive {
        #[arg(long)]
        unit: String,
        #[arg(long)]
        run_id: i64,
        #[arg(long)]
        step: String,
        #[arg(long)]
        reason: String,
        #[arg(long)]
        reviewer: String,
    },
    /// Release review
    Release {
        #[command(subcommand)]
        cmd: ReleaseCmd,
    },
    /// Service records (repairs, part replacement, upgrades)
    Service {
        #[command(subcommand)]
        cmd: ServiceCmd,
    },
    /// Hardware baselines (preserved, never overwritten)
    Baseline {
        #[command(subcommand)]
        cmd: BaselineCmd,
    },
    /// Fault isolation: correlate a stored run's results into observations, hypotheses and next tests
    Analyze {
        #[arg(long)]
        run_id: i64,
        #[arg(long, value_enum, default_value = "text")]
        format: Format,
    },
    /// Automatic follow-up sequence for a stored run (diagnostic only; never release-qualifying)
    FollowUp {
        #[arg(long)]
        run_id: i64,
        /// Execute the follow-up sequence now (otherwise print the generated profile)
        #[arg(long)]
        execute: bool,
        #[arg(long, default_value = "local-operator")]
        operator: String,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Fault-code catalog
    Faults {
        #[arg(long, value_enum, default_value = "text")]
        format: Format,
    },
    /// Append-only audit log
    Audit {
        #[arg(long, default_value_t = 50)]
        limit: u32,
    },
    /// Consistent database backup
    Backup { dest: PathBuf },
    /// Write a redacted support bundle (explicit, user-initiated; never uploaded)
    SupportBundle {
        #[arg(long)]
        out: PathBuf,
    },
}

#[derive(Subcommand)]
enum TestsCmd {
    List {
        #[arg(long, value_enum, default_value = "text")]
        format: Format,
    },
}

#[derive(Subcommand)]
enum ProfileCmd {
    List,
    Show { id: String },
    Validate { path: String },
}

#[derive(clap::Args)]
struct RunArgs {
    /// Built-in profile id (quick, standard, extended-stability, factory-burn-in) or JSON path
    #[arg(long)]
    profile: String,
    /// Manufacturing unit serial (must be registered)
    #[arg(long)]
    unit: Option<String>,
    /// Link this run as rework of an earlier run
    #[arg(long)]
    rework_of: Option<i64>,
    /// Directory for report.html, report.pdf, report.json, results.csv, measurements.csv
    #[arg(long)]
    output: Option<PathBuf>,
    #[arg(long, default_value = "local-operator")]
    operator: String,
    /// Explicitly authorize high-load (stress) tests for this run
    #[arg(long)]
    authorize_high_load: bool,
    /// Approved hardware safety profile (JSON) required for high-load tests
    #[arg(long)]
    safety_profile: Option<PathBuf>,
    /// DEVELOPMENT: enable fixture tests; results are labelled and never releasable
    #[arg(long, hide = true)]
    fixture_mode: bool,
    /// Do not persist to the database
    #[arg(long)]
    no_db: bool,
}

#[derive(Subcommand)]
enum UnitCmd {
    Create {
        #[arg(long)]
        serial: String,
        #[arg(long)]
        configuration: Option<String>,
        #[arg(long)]
        chassis_revision: Option<String>,
        #[arg(long)]
        order_ref: Option<String>,
        #[arg(long)]
        technician: Option<String>,
        /// BOM JSON file
        #[arg(long)]
        bom: Option<PathBuf>,
        #[arg(long, default_value = "local-operator")]
        operator: String,
    },
    Show {
        serial: String,
    },
    List,
}

#[derive(Subcommand)]
enum ReleaseCmd {
    Evaluate {
        #[arg(long)]
        unit: String,
    },
    Approve {
        #[arg(long)]
        unit: String,
        #[arg(long)]
        reviewer: String,
        #[arg(long, default_value = "")]
        note: String,
    },
    /// Place a unit on hold or rework
    Hold {
        #[arg(long)]
        unit: String,
        #[arg(long)]
        actor: String,
        #[arg(long)]
        reason: String,
        #[arg(long)]
        rework: bool,
    },
}

#[derive(Subcommand)]
enum ServiceCmd {
    Add {
        #[arg(long)]
        unit: String,
        #[arg(long)]
        technician: String,
        #[arg(long, value_parser = ["repair", "part-replacement", "upgrade", "inspection", "rework"])]
        kind: String,
        #[arg(long)]
        description: String,
        #[arg(long = "part")]
        parts: Vec<String>,
    },
    List {
        #[arg(long)]
        unit: String,
    },
}

#[derive(Subcommand)]
enum BaselineCmd {
    /// Snapshot the current inventory as a new baseline
    Add {
        #[arg(long)]
        unit: String,
        #[arg(long)]
        note: String,
        #[arg(long, default_value = "local-operator")]
        operator: String,
    },
    /// Compare the current inventory with the latest baseline
    Compare {
        #[arg(long)]
        unit: String,
    },
    List {
        #[arg(long)]
        unit: String,
    },
}

fn main() {
    let cli = Cli::parse();
    let code = match dispatch(cli) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e:#}");
            if e.downcast_ref::<InvalidInput>().is_some() {
                exit::INVALID_INPUT
            } else {
                exit::INTERNAL_ERROR
            }
        }
    };
    std::process::exit(code);
}

#[derive(Debug)]
struct InvalidInput(String);
impl std::fmt::Display for InvalidInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for InvalidInput {}

fn invalid(msg: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(InvalidInput(msg.into()))
}

fn store(db: &Option<PathBuf>) -> Result<Store> {
    be_engine::open_store(db.as_deref()).map_err(|e| anyhow!(e))
}

fn dispatch(cli: Cli) -> Result<i32> {
    let db = cli.db.clone();
    match cli.cmd {
        Cmd::Inventory { format } => {
            let inv = be_platform::native().inventory();
            match format {
                Format::Json => println!("{}", serde_json::to_string_pretty(&inv)?),
                Format::Text => print_inventory(&inv),
            }
            Ok(exit::PASS)
        }
        Cmd::Capabilities { format } => {
            let caps = be_engine::capabilities(be_platform::native().as_ref());
            match format {
                Format::Json => println!("{}", serde_json::to_string_pretty(&caps)?),
                Format::Text => {
                    println!("{:<22} {:<40} {:<18} REASON", "ID", "CAPABILITY", "STATE");
                    for c in caps {
                        println!(
                            "{:<22} {:<40} {:<18} {}",
                            c.id,
                            c.name,
                            c.state.label(),
                            c.state.reason()
                        );
                    }
                }
            }
            Ok(exit::PASS)
        }
        Cmd::Sensors { format, watch } => {
            let p = be_platform::native();
            let mut remaining = watch;
            loop {
                let s = p.sample_sensors();
                match format {
                    Format::Json => println!("{}", serde_json::to_string(&s)?),
                    Format::Text => {
                        if s.is_empty() {
                            println!(
                                "No sensors exposed by this platform (see `bevalidate capabilities`)."
                            );
                        }
                        for r in &s {
                            let v = r
                                .value
                                .map(|v| format!("{v:.1} {}", r.unit))
                                .unwrap_or("unavailable".into());
                            println!(
                                "{:<12} {:<28} {:<12} {}",
                                r.kind.label(),
                                format!("{} {}", r.chip, r.label),
                                v,
                                r.source
                            );
                        }
                    }
                }
                if remaining == 0 {
                    break;
                }
                remaining -= 1;
                std::thread::sleep(std::time::Duration::from_secs(1));
                if format == Format::Text {
                    println!("--- {}", UtcMillis::now().to_rfc3339());
                }
            }
            Ok(exit::PASS)
        }
        Cmd::Tests {
            cmd: TestsCmd::List { format },
        } => {
            let d = be_diagnostics::descriptors(false);
            match format {
                Format::Json => println!("{}", serde_json::to_string_pretty(&d)?),
                Format::Text => {
                    println!(
                        "{:<18} {:<8} {:<40} {:<14} LIMITATIONS",
                        "ID", "VERSION", "NAME", "LOAD"
                    );
                    for t in d {
                        println!(
                            "{:<18} {:<8} {:<40} {:<14} {}",
                            t.id,
                            t.version,
                            t.name,
                            t.load.label(),
                            t.limitations
                        );
                    }
                }
            }
            Ok(exit::PASS)
        }
        Cmd::Profile { cmd } => match cmd {
            ProfileCmd::List => {
                for p in be_engine::builtin_profiles() {
                    println!(
                        "{:<20} {:<8} {:<34} {} steps, {} manual checks",
                        p.id,
                        p.version,
                        p.name,
                        p.steps.len(),
                        p.manual_checks.len()
                    );
                }
                Ok(exit::PASS)
            }
            ProfileCmd::Show { id } => {
                let p = be_engine::load_profile(&id, false).map_err(invalid)?;
                println!("{}", serde_json::to_string_pretty(&p)?);
                Ok(exit::PASS)
            }
            ProfileCmd::Validate { path } => match be_engine::load_profile(&path, false) {
                Ok(p) => {
                    println!("OK: {} {} ({} steps)", p.id, p.version, p.steps.len());
                    Ok(exit::PASS)
                }
                Err(e) => {
                    println!("INVALID: {e}");
                    Ok(exit::INVALID_INPUT)
                }
            },
        },
        Cmd::Run(a) => cmd_run(&db, a),
        Cmd::Report {
            run_id,
            format,
            audience,
            out,
        } => {
            let s = store(&db)?;
            let d = ReportData::from_store(&s, run_id).map_err(invalid)?;
            let aud = if audience == AudienceArg::Customer {
                Audience::Customer
            } else {
                Audience::Internal
            };
            let text = match format {
                ReportFormat::Json => be_reporting::to_json(&d, aud),
                ReportFormat::Csv => be_reporting::results_csv(&d),
                ReportFormat::Html => be_reporting::to_html(&d, aud),
                ReportFormat::Pdf => {
                    let bytes = be_reporting::pdf::to_pdf(&d, aud);
                    match out {
                        Some(p) => write_atomic(&p, &bytes)?,
                        None => {
                            use std::io::Write as _;
                            std::io::stdout().write_all(&bytes)?;
                        }
                    }
                    return Ok(exit::PASS);
                }
            };
            match out {
                Some(p) => write_atomic(&p, text.as_bytes())?,
                None => print!("{text}"),
            }
            Ok(exit::PASS)
        }
        Cmd::Runs { unit, limit } => {
            let s = store(&db)?;
            println!(
                "{:<6} {:<16} {:<20} {:<24} {:<12} {:<12} FLAGS",
                "RUN", "UNIT", "PROFILE", "STARTED (UTC)", "STATE", "OVERALL"
            );
            for r in s.list_runs(unit.as_deref(), limit)? {
                let mut flags = vec![];
                if r.fixture {
                    flags.push("FIXTURE".to_string());
                }
                if let Some(p) = r.parent_run_id {
                    flags.push(format!("rework-of:{p}"));
                }
                println!(
                    "{:<6} {:<16} {:<20} {:<24} {:<12} {:<12} {}",
                    r.id,
                    r.unit_serial.unwrap_or("-".into()),
                    format!("{}@{}", r.profile_id, r.profile_version),
                    r.started_utc.to_rfc3339(),
                    r.state,
                    r.overall_status.unwrap_or("-".into()),
                    flags.join(",")
                );
            }
            Ok(exit::PASS)
        }
        Cmd::Events { run_id, limit } => {
            let s = store(&db)?;
            for e in s.events(run_id, limit)? {
                println!("{} {}", e.ts.date(), e.line());
            }
            Ok(exit::PASS)
        }
        Cmd::Unit { cmd } => cmd_unit(&db, cmd),
        Cmd::Check {
            unit,
            run_id,
            check,
            status,
            operator,
            note,
        } => {
            let s = store(&db)?;
            let run = s
                .get_run(run_id)?
                .ok_or_else(|| invalid(format!("run {run_id} not found")))?;
            if run.unit_serial.as_deref() != Some(unit.as_str()) {
                return Err(invalid(format!(
                    "run {run_id} does not belong to unit {unit}"
                )));
            }
            let profile: be_core::profile::Profile = serde_json::from_str(&run.profile_json)?;
            if !profile.manual_checks.iter().any(|c| c.id == check) {
                return Err(invalid(format!(
                    "profile {} has no manual check `{check}`",
                    profile.id
                )));
            }
            let st = match status.as_str() {
                "pass" => ManualCheckStatus::Pass,
                "fail" => ManualCheckStatus::Fail,
                _ => ManualCheckStatus::NotApplicable,
            };
            s.add_manual_check(
                &unit,
                Some(run_id),
                &ManualCheckRecord {
                    check_id: check,
                    status: st,
                    operator,
                    note,
                },
            )
            .map_err(|e| invalid(e.to_string()))?;
            println!("Recorded.");
            Ok(exit::PASS)
        }
        Cmd::Waive {
            unit,
            run_id,
            step,
            reason,
            reviewer,
        } => {
            let s = store(&db)?;
            s.add_waiver(
                &unit,
                run_id,
                &Waiver {
                    step_key: step,
                    reason,
                    reviewer,
                },
            )
            .map_err(|e| invalid(e.to_string()))?;
            println!(
                "Waiver recorded. It applies only where the profile permits waivers for this step and the result was not a failure."
            );
            Ok(exit::PASS)
        }
        Cmd::Release { cmd } => cmd_release(&db, cmd),
        Cmd::Service { cmd } => {
            let s = store(&db)?;
            match cmd {
                ServiceCmd::Add {
                    unit,
                    technician,
                    kind,
                    description,
                    parts,
                } => {
                    if s.get_unit(&unit)?.is_none() {
                        return Err(invalid(format!("unknown unit {unit}")));
                    }
                    let id = s
                        .add_service_record(&unit, &technician, &kind, &description, &parts)
                        .map_err(|e| invalid(e.to_string()))?;
                    println!(
                        "Service record {id} added. Re-test with `bevalidate run --unit {unit} --rework-of <run>` before release."
                    );
                }
                ServiceCmd::List { unit } => {
                    for r in s.service_records(&unit)? {
                        println!(
                            "{} {:<16} {:<14} {} [{}]",
                            r.ts.to_rfc3339(),
                            r.kind,
                            r.technician,
                            r.description,
                            r.parts.join(", ")
                        );
                    }
                }
            }
            Ok(exit::PASS)
        }
        Cmd::Baseline { cmd } => cmd_baseline(&db, cmd),
        Cmd::Analyze { run_id, format } => {
            let s = store(&db)?;
            let results = s.results_for_run(run_id)?;
            if results.is_empty() {
                return Err(invalid(format!("run {run_id} has no results")));
            }
            let a = be_core::isolation::analyze(&results);
            match format {
                Format::Json => println!("{}", serde_json::to_string_pretty(&a)?),
                Format::Text => print_analysis(&a, &results),
            }
            Ok(exit::PASS)
        }
        Cmd::FollowUp {
            run_id,
            execute,
            operator,
            output,
        } => {
            let s = store(&db)?;
            let run = s
                .get_run(run_id)?
                .ok_or_else(|| invalid(format!("run {run_id} not found")))?;
            let results = s.results_for_run(run_id)?;
            let a = be_core::isolation::analyze(&results);
            let descs = be_diagnostics::descriptors(false);
            let high = |t: &str| {
                descs
                    .iter()
                    .any(|d| d.id == t && d.load == be_core::model::LoadClass::High)
            };
            let Some(p) = be_core::isolation::follow_up_profile(&a, &high, Some(run_id)) else {
                println!("No follow-up tests suggested for run {run_id}.");
                return Ok(exit::PASS);
            };
            if !execute {
                println!("{}", serde_json::to_string_pretty(&p)?);
                eprintln!(
                    "Re-run with --execute to run this diagnostic sequence (results are linked to run {run_id})."
                );
                return Ok(exit::PASS);
            }
            let tmp = std::env::temp_dir()
                .join(format!("be-followup-{}-{run_id}.json", std::process::id()));
            be_core::fsutil::write_atomic(&tmp, serde_json::to_string_pretty(&p)?.as_bytes())?;
            let args = RunArgs {
                profile: tmp.to_string_lossy().into_owned(),
                unit: run.unit_serial.clone(),
                rework_of: if run.unit_serial.is_some() {
                    Some(run_id)
                } else {
                    None
                },
                output,
                operator,
                authorize_high_load: false,
                safety_profile: None,
                fixture_mode: run.fixture,
                no_db: false,
            };
            let r = cmd_run(&db, args);
            let _ = std::fs::remove_file(&tmp);
            r
        }
        Cmd::Faults { format } => {
            match format {
                Format::Json => println!(
                    "{}",
                    serde_json::to_string_pretty(&be_core::codes::CATALOG)?
                ),
                Format::Text => {
                    println!("Fault catalog version {}", be_core::codes::CATALOG_VERSION);
                    for c in be_core::codes::CATALOG {
                        println!(
                            "{:<13} {:<9} {}",
                            c.code,
                            format!("{:?}", c.severity),
                            c.title
                        );
                    }
                }
            }
            Ok(exit::PASS)
        }
        Cmd::Audit { limit } => {
            let s = store(&db)?;
            for a in s.audit_log(limit)? {
                println!(
                    "{} {:<16} {:<20} {}",
                    a.ts.to_rfc3339(),
                    a.actor,
                    a.action,
                    a.details
                );
            }
            Ok(exit::PASS)
        }
        Cmd::Backup { dest } => {
            let s = store(&db)?;
            if dest.exists() {
                return Err(invalid(format!("{} already exists", dest.display())));
            }
            s.backup_to(&dest)?;
            println!("Backup written to {}", dest.display());
            Ok(exit::PASS)
        }
        Cmd::SupportBundle { out } => {
            let p = be_platform::native();
            let mut inv = p.inventory();
            redact_inventory(&mut inv);
            let events: Vec<EventRecord> = store(&db)
                .ok()
                .and_then(|s| s.events(None, 500).ok())
                .unwrap_or_default();
            let bundle = serde_json::json!({
                "bundle_schema": 1,
                "generated_utc": UtcMillis::now().to_rfc3339(),
                "engine_version": be_core::ENGINE_VERSION,
                "redaction": "hostname, serial numbers and MAC addresses removed",
                "inventory": inv,
                "capabilities": be_engine::capabilities(p.as_ref()),
                "recent_events": events,
            });
            write_atomic(&out, serde_json::to_string_pretty(&bundle)?.as_bytes())?;
            println!(
                "Support bundle written to {} (local file only; nothing was uploaded).",
                out.display()
            );
            Ok(exit::PASS)
        }
    }
}

fn redact_inventory(inv: &mut Inventory) {
    inv.system.hostname = None;
    inv.system.serial = None;
    for s in &mut inv.storage {
        s.serial = None;
    }
    for n in &mut inv.network {
        n.mac = None;
    }
}

fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    be_core::fsutil::write_atomic(path, data).with_context(|| format!("writing {}", path.display()))
}

fn cmd_run(db: &Option<PathBuf>, a: RunArgs) -> Result<i32> {
    let profile = be_engine::load_profile(&a.profile, a.fixture_mode).map_err(invalid)?;
    let mut cfg = EngineConfig::new(&a.operator);
    cfg.fixture_mode = a.fixture_mode;
    cfg.auth.high_load = a.authorize_high_load;
    if let Some(sp) = &a.safety_profile {
        let text = be_core::fsutil::read_limited(sp, be_core::fsutil::MAX_CONFIG_BYTES)
            .with_context(|| format!("reading {}", sp.display()))?;
        cfg.auth.safety_profile = Some(
            be_safety::HardwareSafetyProfile::from_json(&text)
                .map_err(|e| invalid(format!("safety profile: {e}")))?,
        );
    }
    let high: Vec<String> = profile
        .steps
        .iter()
        .filter(|s| {
            be_diagnostics::find(&s.test, a.fixture_mode)
                .map(|t| t.descriptor().load == be_core::model::LoadClass::High)
                .unwrap_or(false)
        })
        .map(|s| s.key().to_string())
        .collect();
    if !high.is_empty() {
        eprintln!(
            "NOTICE: profile contains high-load steps: {}.",
            high.join(", ")
        );
        if !a.authorize_high_load {
            eprintln!(
                "        They will NOT run (no --authorize-high-load). Required high-load steps will block release."
            );
        } else {
            eprintln!(
                "        WARNING: authorized high-load testing generates sustained heat and power draw. Monitor the unit; press Ctrl+C to stop."
            );
        }
    }
    let s = if a.no_db { None } else { Some(store(db)?) };
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let c = cancel.clone();
        let _ = ctrlc_hook(move || c.store(true, Ordering::SeqCst));
    }
    let platform = be_platform::native();
    let summary = be_engine::run_profile(
        &cfg,
        &profile,
        a.unit.as_deref(),
        a.rework_of,
        platform.as_ref(),
        s.as_ref(),
        &cancel,
        &mut |e| match e {
            EngineEvent::Log(ev) => {
                eprintln!("{}", ev.line());
            }
            EngineEvent::RunFinished {
                evaluation: Some(ev),
                ..
            } => {
                eprintln!(
                    "Disposition recommendation: {}",
                    ev.recommended_state.label()
                );
                for b in &ev.blockers {
                    eprintln!("  blocker: {b}");
                }
            }
            _ => {}
        },
    )
    .map_err(invalid)?;
    println!("{:<20} {:<14} {:<10} CODE", "STEP", "RESULT", "DURATION");
    for (k, r) in &summary.results {
        println!(
            "{:<20} {:<14} {:<10} {}",
            k,
            r.status.label(),
            be_core::time::format_duration_ms(r.duration_ms),
            r.first_fault_code().unwrap_or("")
        );
    }
    println!("Overall: {}", summary.overall.label());
    if let (Some(dir), Some(st), Some(id)) = (&a.output, &s, summary.run_id) {
        let d = ReportData::from_store(st, id).map_err(|e| anyhow!(e))?;
        std::fs::create_dir_all(dir)?;
        write_atomic(
            &dir.join("report.html"),
            be_reporting::to_html(&d, Audience::Internal).as_bytes(),
        )?;
        write_atomic(
            &dir.join("report.json"),
            be_reporting::to_json(&d, Audience::Internal).as_bytes(),
        )?;
        write_atomic(
            &dir.join("results.csv"),
            be_reporting::results_csv(&d).as_bytes(),
        )?;
        write_atomic(
            &dir.join("measurements.csv"),
            be_reporting::measurements_csv(&d).as_bytes(),
        )?;
        write_atomic(
            &dir.join("report.pdf"),
            &be_reporting::pdf::to_pdf(&d, Audience::Internal),
        )?;
        println!("Reports written to {} (run {id})", dir.display());
    } else if a.output.is_some() {
        eprintln!("Reports require the database (omit --no-db).");
    }
    Ok(exit::for_statuses(
        summary.results.iter().map(|(_, r)| r.status),
    ))
}

/// Minimal Ctrl+C handling without extra dependencies.
fn ctrlc_hook(f: impl Fn() + Send + 'static) -> bool {
    #[cfg(unix)]
    {
        use std::sync::OnceLock;
        static HOOK: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();
        static FLAG: AtomicBool = AtomicBool::new(false);
        extern "C" fn handler(_: libc::c_int) {
            FLAG.store(true, Ordering::SeqCst);
        }
        let f = std::sync::Mutex::new(f);
        let _ = HOOK.set(Box::new(move || (f.lock().unwrap())()));
        // SAFETY: installs an async-signal-safe handler that only stores to an atomic.
        unsafe {
            libc::signal(libc::SIGINT, handler as *const () as libc::sighandler_t);
        }
        std::thread::spawn(|| {
            loop {
                if FLAG.load(Ordering::SeqCst) {
                    if let Some(h) = HOOK.get() {
                        eprintln!("\nStop requested — stopping current test...");
                        h();
                    }
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        });
        true
    }
    #[cfg(not(unix))]
    {
        let _ = f;
        false
    }
}

fn cmd_unit(db: &Option<PathBuf>, cmd: UnitCmd) -> Result<i32> {
    let s = store(db)?;
    match cmd {
        UnitCmd::Create {
            serial,
            configuration,
            chassis_revision,
            order_ref,
            technician,
            bom,
            operator,
        } => {
            let bom_json = match bom {
                Some(p) => {
                    let t = be_core::fsutil::read_limited(&p, be_core::fsutil::MAX_CONFIG_BYTES)?;
                    serde_json::from_str::<serde_json::Value>(&t)
                        .map_err(|e| invalid(format!("BOM is not valid JSON: {e}")))?;
                    Some(t)
                }
                None => None,
            };
            s.create_unit(
                &Unit {
                    serial: serial.clone(),
                    chassis_revision,
                    order_ref,
                    configuration,
                    bom_json,
                    assembly_technician: technician,
                    created_utc: UtcMillis::now(),
                    release_state: ReleaseState::AwaitingTest,
                },
                &operator,
            )
            .map_err(|e| invalid(e.to_string()))?;
            println!("Unit {serial} created (state: Awaiting Test).");
        }
        UnitCmd::Show { serial } => {
            let u = s
                .get_unit(&serial)?
                .ok_or_else(|| invalid(format!("unknown unit {serial}")))?;
            println!("{}", serde_json::to_string_pretty(&u)?);
            println!("Dispositions:");
            for d in s.dispositions(&serial)? {
                println!(
                    "  {} {:<16} by {:<16} {}",
                    d.ts.to_rfc3339(),
                    d.state.label(),
                    d.actor,
                    d.reason
                );
            }
            println!("Runs:");
            for r in s.list_runs(Some(&serial), 20)? {
                println!(
                    "  run {} {}@{} {} {}",
                    r.id,
                    r.profile_id,
                    r.profile_version,
                    r.state,
                    r.overall_status.unwrap_or_default()
                );
            }
        }
        UnitCmd::List => {
            for u in s.list_units()? {
                println!(
                    "{:<20} {:<18} {}",
                    u.serial,
                    u.release_state.label(),
                    u.configuration.unwrap_or_default()
                );
            }
        }
    }
    Ok(exit::PASS)
}

fn cmd_release(db: &Option<PathBuf>, cmd: ReleaseCmd) -> Result<i32> {
    let s = store(db)?;
    match cmd {
        ReleaseCmd::Evaluate { unit } => {
            let (run, e) = be_engine::evaluate_unit(&s, &unit).map_err(invalid)?;
            println!(
                "Unit {unit}, latest run {run}: {}",
                if e.eligible_for_review {
                    "ELIGIBLE for authorized release review"
                } else {
                    "NOT eligible"
                }
            );
            for b in &e.blockers {
                println!("  blocker: {b}");
            }
            for n in &e.notes {
                println!("  note: {n}");
            }
            Ok(if e.eligible_for_review {
                exit::PASS
            } else {
                exit::NOT_AUTHORIZED
            })
        }
        ReleaseCmd::Approve {
            unit,
            reviewer,
            note,
        } => match be_engine::approve_release(&s, &unit, &reviewer, &note) {
            Ok(_) => {
                println!(
                    "Unit {unit} RELEASED by {reviewer}. (Recorded locally; no shipping system integration exists.)"
                );
                Ok(exit::PASS)
            }
            Err(e) => {
                println!("Release REFUSED for {unit}:");
                for b in &e.blockers {
                    println!("  blocker: {b}");
                }
                Ok(exit::NOT_AUTHORIZED)
            }
        },
        ReleaseCmd::Hold {
            unit,
            actor,
            reason,
            rework,
        } => {
            s.record_disposition(&be_storage::Disposition {
                unit_serial: unit.clone(),
                run_id: None,
                state: if rework {
                    ReleaseState::Rework
                } else {
                    ReleaseState::Hold
                },
                actor,
                reason,
                ts: UtcMillis::now(),
            })
            .map_err(|e| invalid(e.to_string()))?;
            println!(
                "Unit {unit} placed on {}.",
                if rework { "Rework" } else { "Hold" }
            );
            Ok(exit::PASS)
        }
    }
}

fn cmd_baseline(db: &Option<PathBuf>, cmd: BaselineCmd) -> Result<i32> {
    let s = store(db)?;
    match cmd {
        BaselineCmd::Add {
            unit,
            note,
            operator,
        } => {
            if s.get_unit(&unit)?.is_none() {
                return Err(invalid(format!("unknown unit {unit}")));
            }
            let inv = be_platform::native().inventory();
            let id =
                s.add_baseline(&unit, None, &serde_json::to_string(&inv)?, &note, &operator)?;
            println!("Baseline {id} recorded for {unit}.");
        }
        BaselineCmd::List { unit } => {
            for b in s.baselines(&unit)? {
                println!(
                    "{:<4} {} run={:?} {}",
                    b.id,
                    b.created_utc.to_rfc3339(),
                    b.run_id,
                    b.note
                );
            }
        }
        BaselineCmd::Compare { unit } => {
            let b = s
                .baselines(&unit)?
                .pop()
                .ok_or_else(|| invalid(format!("no baseline for {unit}")))?;
            let old: Inventory = serde_json::from_str(&b.inventory_json)?;
            let new = be_platform::native().inventory();
            let diffs = compare_inventory(&old, &new);
            if diffs.is_empty() {
                println!(
                    "No hardware differences from baseline {} ({}).",
                    b.id,
                    b.created_utc.to_rfc3339()
                );
                return Ok(exit::PASS);
            }
            println!(
                "Differences from baseline {} ({}):",
                b.id,
                b.created_utc.to_rfc3339()
            );
            for d in &diffs {
                println!("  {d}");
            }
            return Ok(exit::FAIL);
        }
    }
    Ok(exit::PASS)
}

fn compare_inventory(a: &Inventory, b: &Inventory) -> Vec<String> {
    let mut d = Vec::new();
    let cpu = |i: &Inventory| i.cpu.as_ref().and_then(|c| c.model.clone());
    if cpu(a) != cpu(b) {
        d.push(format!("CPU: {:?} -> {:?}", cpu(a), cpu(b)));
    }
    let mem = |i: &Inventory| i.memory.as_ref().map(|m| m.total_bytes / (1 << 20));
    if mem(a) != mem(b) {
        d.push(format!(
            "Memory: {:?} -> {:?}",
            mem(a).map(|m| format_bytes(m << 20)),
            mem(b).map(|m| format_bytes(m << 20))
        ));
    }
    let gpus = |i: &Inventory| {
        i.gpus
            .iter()
            .map(|g| format!("{}:{}", g.id, g.name.clone().unwrap_or_default()))
            .collect::<Vec<_>>()
    };
    if gpus(a) != gpus(b) {
        d.push(format!("GPUs: {:?} -> {:?}", gpus(a), gpus(b)));
    }
    let sto = |i: &Inventory| {
        i.storage
            .iter()
            .map(|s| {
                format!(
                    "{}:{}",
                    s.model.clone().unwrap_or(s.name.clone()),
                    s.serial.clone().unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
    };
    if sto(a) != sto(b) {
        d.push(format!("Storage: {:?} -> {:?}", sto(a), sto(b)));
    }
    if a.firmware.bios_version != b.firmware.bios_version {
        d.push(format!(
            "BIOS: {:?} -> {:?}",
            a.firmware.bios_version, b.firmware.bios_version
        ));
    }
    let pci = |i: &Inventory| {
        i.pci
            .iter()
            .map(|p| format!("{} {:04x?}:{:04x?}", p.address, p.vendor_id, p.device_id))
            .collect::<Vec<_>>()
    };
    if pci(a) != pci(b) {
        d.push("PCI device list changed".into());
    }
    d
}

fn print_analysis(
    a: &be_core::isolation::Analysis,
    results: &[(String, be_core::model::TestResult)],
) {
    println!(
        "Fault isolation (ruleset {}), {} result(s) analysed",
        a.ruleset_version,
        results.len()
    );
    if !be_core::isolation::live_only(results) {
        println!("NOTE: run contains FIXTURE results; findings are for software testing only.");
    }
    if a.findings.is_empty() {
        println!("No correlated faults found.");
        return;
    }
    for f in &a.findings {
        println!(
            "\n[{}] {}{}",
            f.rule,
            f.title,
            if f.hold { "  (hold unit)" } else { "" }
        );
        println!("  Confirmed observations:");
        for o in &f.observations {
            println!("    - {o}");
        }
        println!("  Possible causes (not a diagnosis):");
        for h in &f.hypotheses {
            println!(
                "    - [{}] {}  (evidence: {})",
                h.confidence.label(),
                h.cause,
                h.evidence.join("; ")
            );
        }
        println!("  Next safe tests: {}", f.next_tests.join(", "));
        println!("  Escalation: {}", f.escalation);
    }
    println!("\nFollow-up sequence: {}", a.follow_up.join(" -> "));
    println!("{}", a.caveat);
}

fn print_inventory(inv: &Inventory) {
    println!("Provider: {} ({})", inv.provider, inv.platform);
    println!(
        "Host: {}",
        inv.system.hostname.clone().unwrap_or("unknown".into())
    );
    println!(
        "System: {} {} {}",
        inv.system.manufacturer.clone().unwrap_or("unknown".into()),
        inv.system.product.clone().unwrap_or_default(),
        inv.system
            .virtualized
            .as_ref()
            .map(|v| format!("[virtualized: {v}]"))
            .unwrap_or_default()
    );
    println!(
        "OS: {} / kernel {}",
        inv.os.name.clone().unwrap_or("unknown".into()),
        inv.os.kernel.clone().unwrap_or("unknown".into())
    );
    if let Some(c) = &inv.cpu {
        println!(
            "CPU: {} | {} logical, {} cores, {} package(s) | features: {}",
            c.model.clone().unwrap_or("unknown".into()),
            c.logical_cpus,
            c.physical_cores
                .map(|v| v.to_string())
                .unwrap_or("?".into()),
            c.packages.map(|v| v.to_string()).unwrap_or("?".into()),
            c.features.join(" ")
        );
    }
    if let Some(m) = &inv.memory {
        println!(
            "Memory: {} total, {} available",
            format_bytes(m.total_bytes),
            m.available_bytes
                .map(format_bytes)
                .unwrap_or("unknown".into())
        );
    }
    for s in &inv.storage {
        println!(
            "Storage: {} {} {} [{}]",
            s.name,
            s.model.clone().unwrap_or_default(),
            s.size_bytes
                .map(format_bytes)
                .unwrap_or("size unknown".into()),
            s.transport.clone().unwrap_or_default()
        );
    }
    for g in &inv.gpus {
        println!(
            "GPU: {} {} {} VRAM {}",
            g.id,
            g.vendor.clone().unwrap_or_default(),
            g.name.clone().unwrap_or_default(),
            g.vram_total_bytes
                .map(format_bytes)
                .unwrap_or("unknown".into())
        );
    }
    for n in inv.network.iter().filter(|n| !n.is_virtual) {
        println!(
            "Network: {} {} {}",
            n.name,
            n.oper_state.clone().unwrap_or_default(),
            n.speed_mbps
                .map(|s| format!("{s} Mb/s"))
                .unwrap_or("speed unknown".into())
        );
    }
    println!(
        "PCI functions: {} | USB devices: {} | Sensors: {}",
        inv.pci.len(),
        inv.usb.len(),
        inv.sensors.len()
    );
    println!(
        "Firmware: BIOS {} | UEFI {:?} | Secure Boot {:?} | TPM {:?}",
        inv.firmware
            .bios_version
            .clone()
            .unwrap_or("unknown".into()),
        inv.firmware.uefi,
        inv.firmware.secure_boot,
        inv.firmware.tpm_present
    );
    if !inv.unavailable.is_empty() {
        println!("Unavailable:");
        for u in &inv.unavailable {
            println!("  - {}: {}", u.item, u.reason);
        }
    }
}
