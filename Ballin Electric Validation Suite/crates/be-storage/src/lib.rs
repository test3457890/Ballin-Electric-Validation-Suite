//! SQLite repository for BE Validation: units, runs, immutable results,
//! events, manual checks, waivers, dispositions, service records, baselines
//! and an append-only audit log. All queries are prepared/parameterized.

use be_core::model::{EventLevel, EventRecord, TestResult};
use be_core::policy::{ManualCheckRecord, ManualCheckStatus, ReleaseState, Waiver};
use be_core::time::UtcMillis;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Invalid(String),
    #[error("database schema version {0} is newer than this application supports ({1})")]
    TooNew(i64, i64),
}

pub type Result<T> = std::result::Result<T, StoreError>;

/// Ordered schema migrations. Never edit an applied migration; append.
const MIGRATIONS: &[&str] = &[
    // v1
    r#"
    CREATE TABLE units (
        serial TEXT PRIMARY KEY,
        chassis_revision TEXT,
        order_ref TEXT,
        configuration TEXT,
        bom_json TEXT,
        assembly_technician TEXT,
        created_utc INTEGER NOT NULL,
        release_state TEXT NOT NULL
    );
    CREATE TABLE runs (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        unit_serial TEXT REFERENCES units(serial),
        parent_run_id INTEGER REFERENCES runs(id),
        profile_id TEXT NOT NULL,
        profile_version TEXT NOT NULL,
        profile_json TEXT NOT NULL,
        engine_version TEXT NOT NULL,
        station TEXT NOT NULL,
        operator TEXT NOT NULL,
        started_utc INTEGER NOT NULL,
        ended_utc INTEGER,
        state TEXT NOT NULL,
        overall_status TEXT,
        inventory_json TEXT,
        high_load_authorized INTEGER NOT NULL DEFAULT 0,
        safety_profile_id TEXT,
        fixture INTEGER NOT NULL DEFAULT 0,
        owner_pid INTEGER
    );
    CREATE INDEX runs_unit ON runs(unit_serial);
    CREATE TABLE results (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        run_id INTEGER NOT NULL REFERENCES runs(id),
        step_key TEXT NOT NULL,
        test_id TEXT NOT NULL,
        test_version TEXT NOT NULL,
        status TEXT NOT NULL,
        started_utc INTEGER NOT NULL,
        duration_ms INTEGER NOT NULL,
        result_json TEXT NOT NULL,
        UNIQUE(run_id, step_key)
    );
    CREATE TRIGGER results_immutable_u BEFORE UPDATE ON results BEGIN SELECT RAISE(ABORT, 'results are immutable; create a new run'); END;
    CREATE TRIGGER results_immutable_d BEFORE DELETE ON results BEGIN SELECT RAISE(ABORT, 'results are immutable'); END;
    CREATE TABLE events (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        run_id INTEGER REFERENCES runs(id),
        ts INTEGER NOT NULL,
        level TEXT NOT NULL,
        code TEXT,
        message TEXT NOT NULL
    );
    CREATE INDEX events_run ON events(run_id);
    CREATE TABLE manual_checks (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        unit_serial TEXT NOT NULL REFERENCES units(serial),
        run_id INTEGER REFERENCES runs(id),
        check_id TEXT NOT NULL,
        status TEXT NOT NULL,
        operator TEXT NOT NULL,
        note TEXT NOT NULL,
        ts INTEGER NOT NULL
    );
    CREATE TABLE waivers (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        unit_serial TEXT NOT NULL REFERENCES units(serial),
        run_id INTEGER NOT NULL REFERENCES runs(id),
        step_key TEXT NOT NULL,
        reason TEXT NOT NULL,
        reviewer TEXT NOT NULL,
        ts INTEGER NOT NULL
    );
    CREATE TABLE dispositions (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        unit_serial TEXT NOT NULL REFERENCES units(serial),
        run_id INTEGER REFERENCES runs(id),
        state TEXT NOT NULL,
        actor TEXT NOT NULL,
        reason TEXT NOT NULL,
        ts INTEGER NOT NULL
    );
    CREATE TABLE service_records (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        unit_serial TEXT NOT NULL REFERENCES units(serial),
        ts INTEGER NOT NULL,
        technician TEXT NOT NULL,
        kind TEXT NOT NULL,
        description TEXT NOT NULL,
        parts_json TEXT NOT NULL
    );
    CREATE TABLE baselines (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        unit_serial TEXT NOT NULL REFERENCES units(serial),
        run_id INTEGER REFERENCES runs(id),
        created_utc INTEGER NOT NULL,
        inventory_json TEXT NOT NULL,
        note TEXT NOT NULL
    );
    CREATE TRIGGER baselines_keep_u BEFORE UPDATE ON baselines BEGIN SELECT RAISE(ABORT, 'baselines are preserved'); END;
    CREATE TRIGGER baselines_keep_d BEFORE DELETE ON baselines BEGIN SELECT RAISE(ABORT, 'baselines are preserved'); END;
    CREATE TABLE audit_log (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        ts INTEGER NOT NULL,
        actor TEXT NOT NULL,
        action TEXT NOT NULL,
        details TEXT NOT NULL
    );
    CREATE TRIGGER audit_append_only_u BEFORE UPDATE ON audit_log BEGIN SELECT RAISE(ABORT, 'audit log is append-only'); END;
    CREATE TRIGGER audit_append_only_d BEFORE DELETE ON audit_log BEGIN SELECT RAISE(ABORT, 'audit log is append-only'); END;
    "#,
];

pub const SCHEMA_VERSION: i64 = MIGRATIONS.len() as i64;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Unit {
    pub serial: String,
    pub chassis_revision: Option<String>,
    pub order_ref: Option<String>,
    pub configuration: Option<String>,
    pub bom_json: Option<String>,
    pub assembly_technician: Option<String>,
    pub created_utc: UtcMillis,
    pub release_state: ReleaseState,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewRun {
    pub unit_serial: Option<String>,
    pub parent_run_id: Option<i64>,
    pub profile_id: String,
    pub profile_version: String,
    pub profile_json: String,
    pub station: String,
    pub operator: String,
    pub inventory_json: Option<String>,
    pub high_load_authorized: bool,
    pub safety_profile_id: Option<String>,
    pub fixture: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunRecord {
    pub id: i64,
    pub unit_serial: Option<String>,
    pub parent_run_id: Option<i64>,
    pub profile_id: String,
    pub profile_version: String,
    pub profile_json: String,
    pub engine_version: String,
    pub station: String,
    pub operator: String,
    pub started_utc: UtcMillis,
    pub ended_utc: Option<UtcMillis>,
    pub state: String,
    pub overall_status: Option<String>,
    pub inventory_json: Option<String>,
    pub high_load_authorized: bool,
    pub safety_profile_id: Option<String>,
    pub fixture: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Disposition {
    pub unit_serial: String,
    pub run_id: Option<i64>,
    pub state: ReleaseState,
    pub actor: String,
    pub reason: String,
    pub ts: UtcMillis,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServiceRecord {
    pub id: i64,
    pub unit_serial: String,
    pub ts: UtcMillis,
    pub technician: String,
    pub kind: String,
    pub description: String,
    pub parts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Baseline {
    pub id: i64,
    pub unit_serial: String,
    pub run_id: Option<i64>,
    pub created_utc: UtcMillis,
    pub inventory_json: String,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: i64,
    pub ts: UtcMillis,
    pub actor: String,
    pub action: String,
    pub details: String,
}

pub struct Store {
    conn: Connection,
    path: Option<PathBuf>,
}

fn state_str(s: ReleaseState) -> String {
    serde_json::to_value(s)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default()
}
fn parse_state(s: &str) -> ReleaseState {
    serde_json::from_value(serde_json::Value::String(s.into())).unwrap_or(ReleaseState::Hold)
}
fn check_str(s: ManualCheckStatus) -> String {
    serde_json::to_value(s)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default()
}

/// Default database location (per-user, offline).
pub fn default_db_path() -> PathBuf {
    if let Ok(p) = std::env::var("BE_VALIDATION_DB") {
        return PathBuf::from(p);
    }
    #[cfg(windows)]
    {
        let base = std::env::var("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."));
        base.join("BE Validation").join("be-validation.db")
    }
    #[cfg(not(windows))]
    {
        let base = std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|_| std::env::var("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .unwrap_or_else(|_| PathBuf::from("."));
        base.join("be-validation").join("be-validation.db")
    }
}

fn pid_alive(pid: i64) -> bool {
    #[cfg(target_os = "linux")]
    {
        Path::new(&format!("/proc/{pid}")).exists()
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{CloseHandle, ERROR_INVALID_PARAMETER, WAIT_OBJECT_0};
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
        };
        let Ok(pid) = u32::try_from(pid) else {
            return false;
        };
        // SAFETY: wait on a process handle we own with a zero timeout and close it once.
        unsafe {
            let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
            if handle.is_null() {
                // Access denied or an unknown error does not establish a dead owner.
                return std::io::Error::last_os_error().raw_os_error()
                    != Some(ERROR_INVALID_PARAMETER as i32);
            }
            let alive = WaitForSingleObject(handle, 0) != WAIT_OBJECT_0;
            CloseHandle(handle);
            alive
        }
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = pid;
        true
    }
}

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
        if let Some(dir) = path.parent()
            && !dir.as_os_str().is_empty()
            && !dir.exists()
        {
            std::fs::create_dir_all(dir)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
            }
        }
        let conn = Connection::open(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
        let mut s = Store {
            conn,
            path: Some(path.to_path_buf()),
        };
        s.init()?;
        Ok(s)
    }

    pub fn open_in_memory() -> Result<Store> {
        let mut s = Store {
            conn: Connection::open_in_memory()?,
            path: None,
        };
        s.init()?;
        Ok(s)
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    fn init(&mut self) -> Result<()> {
        self.conn.busy_timeout(std::time::Duration::from_secs(5))?;
        self.conn.pragma_update(None, "foreign_keys", "ON")?;
        if self.path.is_some() {
            self.conn.pragma_update(None, "journal_mode", "WAL")?;
            self.conn.pragma_update(None, "synchronous", "FULL")?;
        }
        let integrity: String = self
            .conn
            .query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        if integrity != "ok" {
            return Err(StoreError::Invalid(format!(
                "database integrity check failed: {integrity}"
            )));
        }
        self.migrate()
    }

    fn migrate(&mut self) -> Result<()> {
        let v: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if v > SCHEMA_VERSION {
            return Err(StoreError::TooNew(v, SCHEMA_VERSION));
        }
        for (i, m) in MIGRATIONS.iter().enumerate().skip(v as usize) {
            let tx = self.conn.transaction()?;
            tx.execute_batch(m)?;
            tx.pragma_update(None, "user_version", (i + 1) as i64)?;
            tx.commit()?;
        }
        Ok(())
    }

    pub fn schema_version(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?)
    }

    // ------------------------------------------------------------- audit
    pub fn audit(&self, actor: &str, action: &str, details: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO audit_log (ts, actor, action, details) VALUES (?1, ?2, ?3, ?4)",
            params![UtcMillis::now().0, actor, action, details],
        )?;
        Ok(())
    }

    pub fn audit_log(&self, limit: u32) -> Result<Vec<AuditEntry>> {
        let mut st = self.conn.prepare(
            "SELECT id, ts, actor, action, details FROM audit_log ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = st.query_map([limit], |r| {
            Ok(AuditEntry {
                id: r.get(0)?,
                ts: UtcMillis(r.get(1)?),
                actor: r.get(2)?,
                action: r.get(3)?,
                details: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    // ------------------------------------------------------------- units
    pub fn create_unit(&self, u: &Unit, actor: &str) -> Result<()> {
        if u.serial.trim().is_empty() {
            return Err(StoreError::Invalid("serial must not be empty".into()));
        }
        self.conn.execute(
            "INSERT INTO units (serial, chassis_revision, order_ref, configuration, bom_json, assembly_technician, created_utc, release_state)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![u.serial, u.chassis_revision, u.order_ref, u.configuration, u.bom_json, u.assembly_technician, u.created_utc.0, state_str(u.release_state)],
        )?;
        self.audit(actor, "unit.create", &format!("serial={}", u.serial))?;
        Ok(())
    }

    fn row_unit(r: &rusqlite::Row) -> rusqlite::Result<Unit> {
        Ok(Unit {
            serial: r.get(0)?,
            chassis_revision: r.get(1)?,
            order_ref: r.get(2)?,
            configuration: r.get(3)?,
            bom_json: r.get(4)?,
            assembly_technician: r.get(5)?,
            created_utc: UtcMillis(r.get(6)?),
            release_state: parse_state(&r.get::<_, String>(7)?),
        })
    }

    pub fn get_unit(&self, serial: &str) -> Result<Option<Unit>> {
        Ok(self
            .conn
            .query_row(
                "SELECT serial, chassis_revision, order_ref, configuration, bom_json, assembly_technician, created_utc, release_state FROM units WHERE serial = ?1",
                [serial],
                Self::row_unit,
            )
            .optional()?)
    }

    pub fn list_units(&self) -> Result<Vec<Unit>> {
        let mut st = self.conn.prepare(
            "SELECT serial, chassis_revision, order_ref, configuration, bom_json, assembly_technician, created_utc, release_state FROM units ORDER BY created_utc DESC",
        )?;
        let rows = st.query_map([], Self::row_unit)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Record a disposition and update the unit's current release state.
    /// Callers must have evaluated the release policy before `Released`.
    pub fn record_disposition(&self, d: &Disposition) -> Result<()> {
        if d.actor.trim().is_empty() {
            return Err(StoreError::Invalid(
                "disposition requires an actor identity".into(),
            ));
        }
        let tx = self.conn.unchecked_transaction()?;
        if d.state == ReleaseState::Released {
            let eligible: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM runs WHERE id = ?1 AND unit_serial = ?2
                 AND state = 'completed' AND fixture = 0
                 AND NOT EXISTS(SELECT 1 FROM runs WHERE unit_serial = ?2 AND (id > ?1 OR state = 'running')))",
                params![d.run_id, d.unit_serial], |row| row.get(0),
            )?;
            if !eligible {
                return Err(StoreError::Invalid(
                    "Release requires the latest completed live run and no active run for the unit"
                        .into(),
                ));
            }
        }
        let n = tx.execute(
            "UPDATE units SET release_state = ?1 WHERE serial = ?2",
            params![state_str(d.state), d.unit_serial],
        )?;
        if n == 0 {
            return Err(StoreError::Invalid(format!(
                "unknown unit {}",
                d.unit_serial
            )));
        }
        tx.execute(
            "INSERT INTO dispositions (unit_serial, run_id, state, actor, reason, ts) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![d.unit_serial, d.run_id, state_str(d.state), d.actor, d.reason, d.ts.0],
        )?;
        tx.execute(
            "INSERT INTO audit_log (ts, actor, action, details) VALUES (?1, ?2, 'unit.disposition', ?3)",
            params![d.ts.0, d.actor, format!("serial={} state={} run={:?} reason={}", d.unit_serial, d.state.label(), d.run_id, d.reason)],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn dispositions(&self, serial: &str) -> Result<Vec<Disposition>> {
        let mut st = self.conn.prepare("SELECT unit_serial, run_id, state, actor, reason, ts FROM dispositions WHERE unit_serial = ?1 ORDER BY id")?;
        let rows = st.query_map([serial], |r| {
            Ok(Disposition {
                unit_serial: r.get(0)?,
                run_id: r.get(1)?,
                state: parse_state(&r.get::<_, String>(2)?),
                actor: r.get(3)?,
                reason: r.get(4)?,
                ts: UtcMillis(r.get(5)?),
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    // -------------------------------------------------------------- runs
    pub fn begin_run(&self, n: &NewRun) -> Result<i64> {
        if let Some(p) = n.parent_run_id {
            let parent_unit: Option<Option<String>> = self
                .conn
                .query_row("SELECT unit_serial FROM runs WHERE id = ?1", [p], |r| {
                    r.get(0)
                })
                .optional()?;
            match parent_unit {
                None => {
                    return Err(StoreError::Invalid(format!(
                        "parent run {p} does not exist"
                    )));
                }
                Some(u) if u != n.unit_serial => {
                    return Err(StoreError::Invalid(
                        "rework run must belong to the same unit as its parent".into(),
                    ));
                }
                _ => {}
            }
        }
        self.conn.execute(
            "INSERT INTO runs (unit_serial, parent_run_id, profile_id, profile_version, profile_json, engine_version, station, operator,
                               started_utc, state, inventory_json, high_load_authorized, safety_profile_id, fixture, owner_pid)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'running', ?10, ?11, ?12, ?13, ?14)",
            params![
                n.unit_serial,
                n.parent_run_id,
                n.profile_id,
                n.profile_version,
                n.profile_json,
                be_core::ENGINE_VERSION,
                n.station,
                n.operator,
                UtcMillis::now().0,
                n.inventory_json,
                n.high_load_authorized,
                n.safety_profile_id,
                n.fixture,
                std::process::id() as i64
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        self.audit(
            &n.operator,
            "run.begin",
            &format!(
                "run={id} profile={}@{} unit={:?} fixture={}",
                n.profile_id, n.profile_version, n.unit_serial, n.fixture
            ),
        )?;
        Ok(id)
    }

    pub fn finish_run(&self, id: i64, state: &str, overall: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE runs SET ended_utc = ?1, state = ?2, overall_status = ?3 WHERE id = ?4 AND state = 'running'",
            params![UtcMillis::now().0, state, overall, id],
        )?;
        Ok(())
    }

    /// Mark runs left in `running` by a process that no longer exists as
    /// interrupted (unexpected shutdown, crash, power loss). Never passed.
    pub fn recover_interrupted(&self) -> Result<Vec<i64>> {
        let mut st = self
            .conn
            .prepare("SELECT id, owner_pid FROM runs WHERE state = 'running'")?;
        let rows: Vec<(i64, Option<i64>)> = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;
        let mut out = Vec::new();
        for (id, pid) in rows {
            let alive = pid
                .map(|p| p == std::process::id() as i64 || pid_alive(p))
                .unwrap_or(false);
            if alive {
                continue;
            }
            self.conn.execute(
                "UPDATE runs SET state = 'interrupted', overall_status = 'ABORTED', ended_utc = ?1 WHERE id = ?2 AND state = 'running'",
                params![UtcMillis::now().0, id],
            )?;
            self.add_event(
                Some(id),
                &EventRecord::new(EventLevel::Error, "Run did not complete (application exit, crash, hang or power loss); results are incomplete").with_code("BE-PWR-001"),
            )?;
            self.audit("system", "run.interrupted", &format!("run={id}"))?;
            out.push(id);
        }
        Ok(out)
    }

    fn row_run(r: &rusqlite::Row) -> rusqlite::Result<RunRecord> {
        Ok(RunRecord {
            id: r.get(0)?,
            unit_serial: r.get(1)?,
            parent_run_id: r.get(2)?,
            profile_id: r.get(3)?,
            profile_version: r.get(4)?,
            profile_json: r.get(5)?,
            engine_version: r.get(6)?,
            station: r.get(7)?,
            operator: r.get(8)?,
            started_utc: UtcMillis(r.get(9)?),
            ended_utc: r.get::<_, Option<i64>>(10)?.map(UtcMillis),
            state: r.get(11)?,
            overall_status: r.get(12)?,
            inventory_json: r.get(13)?,
            high_load_authorized: r.get(14)?,
            safety_profile_id: r.get(15)?,
            fixture: r.get(16)?,
        })
    }

    const RUN_COLS: &'static str = "id, unit_serial, parent_run_id, profile_id, profile_version, profile_json, engine_version, station, operator,
        started_utc, ended_utc, state, overall_status, inventory_json, high_load_authorized, safety_profile_id, fixture";

    pub fn get_run(&self, id: i64) -> Result<Option<RunRecord>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {} FROM runs WHERE id = ?1", Self::RUN_COLS),
                [id],
                Self::row_run,
            )
            .optional()?)
    }

    pub fn list_runs(&self, unit: Option<&str>, limit: u32) -> Result<Vec<RunRecord>> {
        let sql = format!(
            "SELECT {} FROM runs WHERE (?1 IS NULL OR unit_serial = ?1) ORDER BY id DESC LIMIT ?2",
            Self::RUN_COLS
        );
        let mut st = self.conn.prepare(&sql)?;
        let rows = st.query_map(params![unit, limit], Self::row_run)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Latest run only if completed and no run for the same unit is active.
    pub fn latest_run_for_unit(&self, unit: &str) -> Result<Option<RunRecord>> {
        Ok(self
            .conn
            .query_row(
                &format!(
                    "SELECT {} FROM runs WHERE unit_serial = ?1 AND state = 'completed'
                AND id = (SELECT MAX(id) FROM runs WHERE unit_serial = ?1)
                AND NOT EXISTS(SELECT 1 FROM runs WHERE unit_serial = ?1 AND state = 'running')",
                    Self::RUN_COLS
                ),
                [unit],
                Self::row_run,
            )
            .optional()?)
    }

    // ----------------------------------------------------------- results
    pub fn insert_result(&self, run_id: i64, step_key: &str, r: &TestResult) -> Result<()> {
        self.conn.execute(
            "INSERT INTO results (run_id, step_key, test_id, test_version, status, started_utc, duration_ms, result_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![run_id, step_key, r.test_id, r.test_version, r.status.label(), r.started_utc.0, r.duration_ms as i64, serde_json::to_string(r)?],
        )?;
        Ok(())
    }

    pub fn results_for_run(&self, run_id: i64) -> Result<Vec<(String, TestResult)>> {
        let mut st = self
            .conn
            .prepare("SELECT step_key, result_json FROM results WHERE run_id = ?1 ORDER BY id")?;
        let rows = st.query_map([run_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (k, j) = row?;
            out.push((k, serde_json::from_str(&j)?));
        }
        Ok(out)
    }

    // ------------------------------------------------------------ events
    pub fn add_event(&self, run_id: Option<i64>, e: &EventRecord) -> Result<()> {
        self.conn.execute(
            "INSERT INTO events (run_id, ts, level, code, message) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![run_id, e.ts.0, e.level.label(), e.code, e.message],
        )?;
        Ok(())
    }

    pub fn events(&self, run_id: Option<i64>, limit: u32) -> Result<Vec<EventRecord>> {
        let mut st = self.conn.prepare(
            "SELECT ts, level, code, message FROM (SELECT * FROM events WHERE (?1 IS NULL OR run_id = ?1) ORDER BY id DESC LIMIT ?2) ORDER BY id",
        )?;
        let rows = st.query_map(params![run_id, limit], |r| {
            Ok(EventRecord {
                ts: UtcMillis(r.get(0)?),
                level: EventLevel::parse(&r.get::<_, String>(1)?).unwrap_or(EventLevel::Info),
                code: r.get(2)?,
                message: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    // ----------------------------------------------- manual checks/waivers
    pub fn add_manual_check(
        &self,
        serial: &str,
        run_id: Option<i64>,
        m: &ManualCheckRecord,
    ) -> Result<()> {
        if m.operator.trim().is_empty() {
            return Err(StoreError::Invalid(
                "manual check sign-off requires an operator identity".into(),
            ));
        }
        self.conn.execute(
            "INSERT INTO manual_checks (unit_serial, run_id, check_id, status, operator, note, ts) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![serial, run_id, m.check_id, check_str(m.status), m.operator, m.note, UtcMillis::now().0],
        )?;
        self.audit(
            &m.operator,
            "manual_check",
            &format!("serial={serial} check={} status={:?}", m.check_id, m.status),
        )?;
        Ok(())
    }

    /// Manual checks for a unit, optionally restricted to one run.
    pub fn manual_checks(
        &self,
        serial: &str,
        run_id: Option<i64>,
    ) -> Result<Vec<ManualCheckRecord>> {
        let mut st = self.conn.prepare(
            "SELECT check_id, status, operator, note FROM manual_checks WHERE unit_serial = ?1 AND (?2 IS NULL OR run_id = ?2) ORDER BY id",
        )?;
        let rows = st.query_map(params![serial, run_id], |r| {
            let s: String = r.get(1)?;
            Ok(ManualCheckRecord {
                check_id: r.get(0)?,
                status: serde_json::from_value(serde_json::Value::String(s))
                    .unwrap_or(ManualCheckStatus::NotDone),
                operator: r.get(2)?,
                note: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn add_waiver(&self, serial: &str, run_id: i64, w: &Waiver) -> Result<()> {
        if w.reviewer.trim().is_empty() || w.reason.trim().is_empty() {
            return Err(StoreError::Invalid(
                "waiver requires reviewer identity and reason".into(),
            ));
        }
        self.conn.execute(
            "INSERT INTO waivers (unit_serial, run_id, step_key, reason, reviewer, ts) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![serial, run_id, w.step_key, w.reason, w.reviewer, UtcMillis::now().0],
        )?;
        self.audit(
            &w.reviewer,
            "waiver",
            &format!(
                "serial={serial} run={run_id} step={} reason={}",
                w.step_key, w.reason
            ),
        )?;
        Ok(())
    }

    pub fn waivers(&self, serial: &str, run_id: i64) -> Result<Vec<Waiver>> {
        let mut st = self.conn.prepare("SELECT step_key, reason, reviewer FROM waivers WHERE unit_serial = ?1 AND run_id = ?2 ORDER BY id")?;
        let rows = st.query_map(params![serial, run_id], |r| {
            Ok(Waiver {
                step_key: r.get(0)?,
                reason: r.get(1)?,
                reviewer: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    // --------------------------------------------- service and baselines
    pub fn add_service_record(
        &self,
        serial: &str,
        technician: &str,
        kind: &str,
        description: &str,
        parts: &[String],
    ) -> Result<i64> {
        if technician.trim().is_empty() {
            return Err(StoreError::Invalid(
                "service record requires a technician".into(),
            ));
        }
        self.conn.execute(
            "INSERT INTO service_records (unit_serial, ts, technician, kind, description, parts_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![serial, UtcMillis::now().0, technician, kind, description, serde_json::to_string(parts)?],
        )?;
        let id = self.conn.last_insert_rowid();
        self.audit(
            technician,
            "service.record",
            &format!("serial={serial} kind={kind} id={id}"),
        )?;
        Ok(id)
    }

    pub fn service_records(&self, serial: &str) -> Result<Vec<ServiceRecord>> {
        let mut st = self.conn.prepare("SELECT id, unit_serial, ts, technician, kind, description, parts_json FROM service_records WHERE unit_serial = ?1 ORDER BY id")?;
        let rows = st.query_map([serial], |r| {
            Ok(ServiceRecord {
                id: r.get(0)?,
                unit_serial: r.get(1)?,
                ts: UtcMillis(r.get(2)?),
                technician: r.get(3)?,
                kind: r.get(4)?,
                description: r.get(5)?,
                parts: serde_json::from_str(&r.get::<_, String>(6)?).unwrap_or_default(),
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn add_baseline(
        &self,
        serial: &str,
        run_id: Option<i64>,
        inventory_json: &str,
        note: &str,
        actor: &str,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO baselines (unit_serial, run_id, created_utc, inventory_json, note) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![serial, run_id, UtcMillis::now().0, inventory_json, note],
        )?;
        let id = self.conn.last_insert_rowid();
        self.audit(actor, "baseline.add", &format!("serial={serial} id={id}"))?;
        Ok(id)
    }

    pub fn baselines(&self, serial: &str) -> Result<Vec<Baseline>> {
        let mut st = self.conn.prepare("SELECT id, unit_serial, run_id, created_utc, inventory_json, note FROM baselines WHERE unit_serial = ?1 ORDER BY id")?;
        let rows = st.query_map([serial], |r| {
            Ok(Baseline {
                id: r.get(0)?,
                unit_serial: r.get(1)?,
                run_id: r.get(2)?,
                created_utc: UtcMillis(r.get(3)?),
                inventory_json: r.get(4)?,
                note: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Consistent online backup to `dest` (for export/recovery).
    pub fn backup_to(&self, dest: &Path) -> Result<()> {
        self.conn
            .execute("VACUUM INTO ?1", [dest.to_string_lossy()])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use be_core::model::Provenance;
    use be_core::status::TestStatus;

    fn unit(serial: &str) -> Unit {
        Unit {
            serial: serial.into(),
            chassis_revision: Some("A".into()),
            order_ref: None,
            configuration: Some("BE-01 dual GPU".into()),
            bom_json: None,
            assembly_technician: Some("tech1".into()),
            created_utc: UtcMillis::now(),
            release_state: ReleaseState::AwaitingTest,
        }
    }

    fn new_run(serial: Option<&str>, parent: Option<i64>) -> NewRun {
        NewRun {
            unit_serial: serial.map(String::from),
            parent_run_id: parent,
            profile_id: "quick".into(),
            profile_version: "1.0.0".into(),
            profile_json: "{}".into(),
            station: "bench".into(),
            operator: "op".into(),
            inventory_json: None,
            high_load_authorized: false,
            safety_profile_id: None,
            fixture: false,
        }
    }

    #[test]
    fn migrations_and_round_trip() {
        let s = Store::open_in_memory().unwrap();
        assert_eq!(s.schema_version().unwrap(), SCHEMA_VERSION);
        s.create_unit(&unit("BE01-T1"), "op").unwrap();
        assert!(
            s.create_unit(&unit("BE01-T1"), "op").is_err(),
            "duplicate serial"
        );
        let id = s.begin_run(&new_run(Some("BE01-T1"), None)).unwrap();
        let mut r = TestResult::not_executed(
            "BE-T-X",
            "1",
            TestStatus::Unsupported,
            "n/a",
            Provenance::Live,
        );
        r.duration_ms = 5;
        s.insert_result(id, "BE-T-X", &r).unwrap();
        assert!(
            s.insert_result(id, "BE-T-X", &r).is_err(),
            "one result per step per run"
        );
        let back = s.results_for_run(id).unwrap();
        assert_eq!(back[0].1, r);
        s.finish_run(id, "completed", "UNSUPPORTED").unwrap();
        let run = s.get_run(id).unwrap().unwrap();
        assert_eq!(run.state, "completed");
        assert_eq!(run.engine_version, be_core::ENGINE_VERSION);
    }

    #[test]
    fn results_and_audit_are_immutable() {
        let s = Store::open_in_memory().unwrap();
        let id = s.begin_run(&new_run(None, None)).unwrap();
        let r = TestResult::not_executed("X", "1", TestStatus::Fail, "", Provenance::Live);
        s.insert_result(id, "X", &r).unwrap();
        assert!(
            s.conn
                .execute("UPDATE results SET status = 'PASS'", [])
                .is_err()
        );
        assert!(s.conn.execute("DELETE FROM results", []).is_err());
        assert!(s.conn.execute("DELETE FROM audit_log", []).is_err());
        assert!(!s.audit_log(10).unwrap().is_empty());
    }

    #[test]
    fn rework_links_to_parent_of_same_unit() {
        let s = Store::open_in_memory().unwrap();
        s.create_unit(&unit("A"), "op").unwrap();
        s.create_unit(&unit("B"), "op").unwrap();
        let p = s.begin_run(&new_run(Some("A"), None)).unwrap();
        assert!(s.begin_run(&new_run(Some("A"), Some(p))).is_ok());
        assert!(s.begin_run(&new_run(Some("B"), Some(p))).is_err());
        assert!(s.begin_run(&new_run(Some("A"), Some(999))).is_err());
    }

    #[test]
    fn interrupted_runs_are_recovered_never_passed() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("x.db");
        let id = {
            let s = Store::open(&db).unwrap();
            let id = s.begin_run(&new_run(None, None)).unwrap();
            // Simulate a crashed owner process.
            s.conn
                .execute("UPDATE runs SET owner_pid = 999999999 WHERE id = ?1", [id])
                .unwrap();
            id
        };
        let s = Store::open(&db).unwrap();
        assert_eq!(s.recover_interrupted().unwrap(), vec![id]);
        let run = s.get_run(id).unwrap().unwrap();
        assert_eq!(run.state, "interrupted");
        assert_eq!(run.overall_status.as_deref(), Some("ABORTED"));
        assert!(
            s.events(Some(id), 10)
                .unwrap()
                .iter()
                .any(|e| e.code.as_deref() == Some("BE-PWR-001"))
        );
        // A live run owned by this process is not touched.
        let live = s.begin_run(&new_run(None, None)).unwrap();
        assert!(s.recover_interrupted().unwrap().is_empty());
        assert_eq!(s.get_run(live).unwrap().unwrap().state, "running");
    }

    #[test]
    fn active_or_interrupted_rework_blocks_release_of_an_older_run() {
        let s = Store::open_in_memory().unwrap();
        s.create_unit(&unit("U"), "op").unwrap();
        let first = s.begin_run(&new_run(Some("U"), None)).unwrap();
        s.finish_run(first, "completed", "PASS").unwrap();
        assert_eq!(s.latest_run_for_unit("U").unwrap().unwrap().id, first);
        let next = s.begin_run(&new_run(Some("U"), Some(first))).unwrap();
        let release = Disposition {
            unit_serial: "U".into(),
            run_id: Some(first),
            state: ReleaseState::Released,
            actor: "qa".into(),
            reason: "reviewed".into(),
            ts: UtcMillis::now(),
        };
        assert!(s.latest_run_for_unit("U").unwrap().is_none());
        assert!(s.record_disposition(&release).is_err());
        s.finish_run(next, "interrupted", "ABORTED").unwrap();
        assert!(s.latest_run_for_unit("U").unwrap().is_none());
        assert!(s.record_disposition(&release).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn windows_identifies_a_live_process_owned_by_another_instance() {
        use std::process::{Command, Stdio};
        let mut child = Command::new("cmd.exe")
            .arg("/D")
            .arg("/Q")
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        let alive = pid_alive(i64::from(child.id()));
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(alive);
    }

    #[test]
    fn dispositions_checks_waivers_service_baselines() {
        let s = Store::open_in_memory().unwrap();
        s.create_unit(&unit("U"), "op").unwrap();
        let run = s.begin_run(&new_run(Some("U"), None)).unwrap();
        s.record_disposition(&Disposition {
            unit_serial: "U".into(),
            run_id: Some(run),
            state: ReleaseState::Hold,
            actor: "qa".into(),
            reason: "fail".into(),
            ts: UtcMillis::now(),
        })
        .unwrap();
        assert_eq!(
            s.get_unit("U").unwrap().unwrap().release_state,
            ReleaseState::Hold
        );
        assert!(
            s.record_disposition(&Disposition {
                unit_serial: "U".into(),
                run_id: None,
                state: ReleaseState::Released,
                actor: " ".into(),
                reason: "".into(),
                ts: UtcMillis::now()
            })
            .is_err()
        );
        s.add_manual_check(
            "U",
            Some(run),
            &ManualCheckRecord {
                check_id: "finish".into(),
                status: ManualCheckStatus::Pass,
                operator: "op".into(),
                note: String::new(),
            },
        )
        .unwrap();
        assert_eq!(s.manual_checks("U", Some(run)).unwrap().len(), 1);
        assert!(
            s.add_waiver(
                "U",
                run,
                &Waiver {
                    step_key: "x".into(),
                    reason: "".into(),
                    reviewer: "qa".into()
                }
            )
            .is_err()
        );
        s.add_waiver(
            "U",
            run,
            &Waiver {
                step_key: "x".into(),
                reason: "no fixture".into(),
                reviewer: "qa".into(),
            },
        )
        .unwrap();
        assert_eq!(s.waivers("U", run).unwrap().len(), 1);
        s.add_service_record(
            "U",
            "tech",
            "repair",
            "replaced riser",
            &["RISER-16X".into()],
        )
        .unwrap();
        assert_eq!(
            s.service_records("U").unwrap()[0].parts,
            vec!["RISER-16X".to_string()]
        );
        s.add_baseline("U", Some(run), "{}", "factory", "op")
            .unwrap();
        s.add_baseline("U", None, "{}", "after upgrade", "op")
            .unwrap();
        assert_eq!(
            s.baselines("U").unwrap().len(),
            2,
            "baselines preserved, never overwritten"
        );
        assert!(s.conn.execute("DELETE FROM baselines", []).is_err());
    }

    #[test]
    fn newer_schema_is_rejected_and_backup_works() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("y.db");
        {
            let s = Store::open(&db).unwrap();
            s.backup_to(&dir.path().join("backup.db")).unwrap();
            s.conn.pragma_update(None, "user_version", 99).unwrap();
        }
        assert!(matches!(Store::open(&db), Err(StoreError::TooNew(99, _))));
        let b = Store::open(&dir.path().join("backup.db")).unwrap();
        assert_eq!(b.schema_version().unwrap(), SCHEMA_VERSION);
    }
}
