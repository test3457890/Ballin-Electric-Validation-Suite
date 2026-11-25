//! Isolated worker supervision: spawn, bounded IPC, heartbeats, timeouts,
//! cancellation, safety-monitor aborts and process-tree termination.
//!
//! Linux: the worker runs in its own process group, killed with SIGKILL as a
//! group on abort; the worker also sets PR_SET_PDEATHSIG. Windows: the child
//! is killed directly; Job Object containment of grandchildren is planned
//! (current workers spawn no children).

use crate::watchdog::{SafetyMonitor, SafetyVerdict};
use be_core::ipc::{MAX_LINE_BYTES, PROTOCOL_VERSION, WorkerMessage, WorkerRequest};
use be_core::model::{EventLevel, FaultRecord, Provenance, TestResult};
use be_core::status::TestStatus;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct SupervisorConfig {
    pub worker_path: PathBuf,
    /// Hard limit for the whole test.
    pub timeout: Duration,
    /// Maximum silence between worker messages.
    pub heartbeat_timeout: Duration,
    /// Time allowed for cooperative cancellation before forced kill.
    pub cancel_grace: Duration,
    pub fixture_mode: bool,
    pub high_load_authorized: bool,
}

impl SupervisorConfig {
    pub fn new(worker_path: PathBuf, timeout: Duration) -> Self {
        SupervisorConfig {
            worker_path,
            timeout,
            heartbeat_timeout: Duration::from_secs(10),
            cancel_grace: Duration::from_secs(3),
            fixture_mode: false,
            high_load_authorized: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SupervisorEvent {
    Started {
        pid: u32,
    },
    Progress {
        progress: Option<f64>,
        phase: String,
    },
    Log {
        level: EventLevel,
        code: Option<String>,
        message: String,
    },
}

enum Inbound {
    Msg(WorkerMessage),
    Bad(String),
    Eof,
}

/// Locate the worker executable next to the current executable, or via the
/// `BE_WORKER_PATH` environment variable.
pub fn default_worker_path() -> PathBuf {
    if let Ok(p) = std::env::var("BE_WORKER_PATH") {
        return PathBuf::from(p);
    }
    let name = if cfg!(windows) {
        "bevalidate-worker.exe"
    } else {
        "bevalidate-worker"
    };
    std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join(name)))
        .unwrap_or_else(|| PathBuf::from(name))
}

fn kill_tree(child: &mut Child) {
    #[cfg(unix)]
    {
        let pgid = child.id() as i32;
        // SAFETY: sends SIGKILL to the worker's own process group (created
        // with process_group(0) at spawn, so pgid == worker pid).
        unsafe {
            libc::kill(-pgid, libc::SIGKILL);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn read_bounded_line<R: BufRead>(r: &mut R, buf: &mut Vec<u8>) -> std::io::Result<Option<bool>> {
    // Returns Ok(None) on EOF, Ok(Some(true)) for a line, Ok(Some(false)) if oversized.
    buf.clear();
    let n = r
        .by_ref()
        .take(MAX_LINE_BYTES as u64 + 1)
        .read_until(b'\n', buf)?;
    if n == 0 {
        return Ok(None);
    }
    if buf.last() != Some(&b'\n') && buf.len() > MAX_LINE_BYTES {
        return Ok(Some(false));
    }
    Ok(Some(true))
}

fn failed(
    test_id: &str,
    status: TestStatus,
    code: &str,
    title: &str,
    detail: String,
    prov: Provenance,
    started: Instant,
) -> TestResult {
    let mut r = TestResult::not_executed(test_id, "", status, detail.clone(), prov);
    r.faults.push(FaultRecord::new(code, title, detail));
    r.duration_ms = started.elapsed().as_millis() as u64;
    r
}

/// Run one test in an isolated worker and return its (always terminal) result.
pub fn run_supervised(
    cfg: &SupervisorConfig,
    test_id: &str,
    params: &serde_json::Value,
    cancel: &AtomicBool,
    mut monitor: Option<&mut dyn SafetyMonitor>,
    on_event: &mut dyn FnMut(SupervisorEvent),
) -> TestResult {
    let started = Instant::now();
    let prov = if cfg.fixture_mode {
        Provenance::Fixture
    } else {
        Provenance::Live
    };
    let mut cmd = Command::new(&cfg.worker_path);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return failed(
                test_id,
                TestStatus::NotRun,
                "BE-SYS-001",
                "Worker could not be started",
                format!("{}: {e}", cfg.worker_path.display()),
                prov,
                started,
            );
        }
    };
    on_event(SupervisorEvent::Started { pid: child.id() });
    let mut stdin = child.stdin.take().expect("piped");
    let req = WorkerRequest {
        protocol: PROTOCOL_VERSION,
        test_id: test_id.into(),
        params: params.clone(),
        fixture_mode: cfg.fixture_mode,
        high_load_authorized: cfg.high_load_authorized,
    };
    let mut line = serde_json::to_string(&req).expect("serializable");
    line.push('\n');
    if stdin
        .write_all(line.as_bytes())
        .and_then(|_| stdin.flush())
        .is_err()
    {
        kill_tree(&mut child);
        return failed(
            test_id,
            TestStatus::Aborted,
            "BE-SYS-001",
            "Worker closed its input",
            "request could not be delivered".into(),
            prov,
            started,
        );
    }

    // Bounded channel: the reader blocks if the supervisor falls behind.
    let (tx, rx) = mpsc::sync_channel::<Inbound>(256);
    let stdout = child.stdout.take().expect("piped");
    std::thread::spawn(move || {
        let mut r = BufReader::new(stdout);
        let mut buf = Vec::new();
        loop {
            match read_bounded_line(&mut r, &mut buf) {
                Ok(None) | Err(_) => {
                    let _ = tx.send(Inbound::Eof);
                    return;
                }
                Ok(Some(false)) => {
                    let _ = tx.send(Inbound::Bad(format!(
                        "protocol line exceeded {MAX_LINE_BYTES} bytes"
                    )));
                    return;
                }
                Ok(Some(true)) => {
                    let msg = match serde_json::from_slice::<WorkerMessage>(&buf) {
                        Ok(m) => Inbound::Msg(m),
                        Err(e) => Inbound::Bad(format!("malformed protocol line: {e}")),
                    };
                    if tx.send(msg).is_err() {
                        return;
                    }
                }
            }
        }
    });
    // Bounded stderr capture, preserved on failure.
    let stderr = child.stderr.take().expect("piped");
    let stderr_handle = std::thread::spawn(move || {
        let mut s = Vec::new();
        let _ = stderr.take(64 * 1024).read_to_end(&mut s);
        String::from_utf8_lossy(&s).into_owned()
    });

    let deadline = started + cfg.timeout;
    let mut last_msg = Instant::now();
    let mut cancel_sent: Option<Instant> = None;
    let mut safety_abort: Option<(String, String)> = None;
    let mut warnings: Vec<FaultRecord> = Vec::new();
    let mut next_sample = Instant::now();
    let tick = Duration::from_millis(50);

    let outcome: Result<TestResult, (TestStatus, &str, &str, String)> = loop {
        // Safety monitoring.
        if let Some(m) = monitor.as_deref_mut() {
            if Instant::now() >= next_sample && safety_abort.is_none() {
                next_sample = Instant::now() + Duration::from_millis(m.sample_interval_ms());
                for v in m.check() {
                    match v {
                        SafetyVerdict::Ok => {}
                        SafetyVerdict::Warn { code, message } => {
                            on_event(SupervisorEvent::Log {
                                level: EventLevel::Warn,
                                code: Some(code.clone()),
                                message: message.clone(),
                            });
                            warnings.push(FaultRecord::new(
                                &code,
                                "Safety warning during test",
                                message,
                            ));
                        }
                        SafetyVerdict::Abort { code, message } => {
                            on_event(SupervisorEvent::Log {
                                level: EventLevel::Error,
                                code: Some(code.clone()),
                                message: format!("SAFETY ABORT: {message}"),
                            });
                            safety_abort = Some((code, message));
                        }
                    }
                }
            }
        }
        let want_cancel = cancel.load(Ordering::SeqCst) || safety_abort.is_some();
        if want_cancel && cancel_sent.is_none() {
            let _ = stdin.write_all(b"cancel\n");
            let _ = stdin.flush();
            cancel_sent = Some(Instant::now());
        }
        if let Some(t) = cancel_sent {
            if t.elapsed() > cfg.cancel_grace {
                kill_tree(&mut child);
                break Err((
                    TestStatus::Aborted,
                    "BE-SYS-003",
                    "Worker terminated after cancellation grace period",
                    "forced termination".into(),
                ));
            }
        }
        if Instant::now() > deadline {
            kill_tree(&mut child);
            break Err((
                TestStatus::Aborted,
                "BE-SYS-002",
                "Test exceeded its time limit",
                format!("timeout after {:?}", cfg.timeout),
            ));
        }
        if last_msg.elapsed() > cfg.heartbeat_timeout {
            kill_tree(&mut child);
            break Err((
                TestStatus::Aborted,
                "BE-SYS-002",
                "Worker stopped sending heartbeats",
                format!("no message for {:?}", cfg.heartbeat_timeout),
            ));
        }
        match rx.recv_timeout(tick) {
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) | Ok(Inbound::Eof) => {
                let status = child.wait().ok();
                break Err((
                    TestStatus::Fail,
                    "BE-SYS-001",
                    "Worker exited without a result",
                    format!(
                        "exit status: {}",
                        status.map(|s| s.to_string()).unwrap_or("unknown".into())
                    ),
                ));
            }
            Ok(Inbound::Bad(e)) => {
                kill_tree(&mut child);
                break Err((
                    TestStatus::Inconclusive,
                    "BE-SYS-001",
                    "Worker protocol violation",
                    e,
                ));
            }
            Ok(Inbound::Msg(m)) => {
                last_msg = Instant::now();
                match m {
                    WorkerMessage::Hello { protocol, .. } if protocol != PROTOCOL_VERSION => {
                        kill_tree(&mut child);
                        break Err((
                            TestStatus::Inconclusive,
                            "BE-SYS-001",
                            "Worker protocol mismatch",
                            format!("worker protocol {protocol}"),
                        ));
                    }
                    WorkerMessage::Hello { .. } => {}
                    WorkerMessage::Heartbeat { progress, phase } => {
                        on_event(SupervisorEvent::Progress { progress, phase })
                    }
                    WorkerMessage::Log {
                        level,
                        code,
                        message,
                    } => on_event(SupervisorEvent::Log {
                        level,
                        code,
                        message,
                    }),
                    WorkerMessage::Error { message } => {
                        kill_tree(&mut child);
                        break Err((
                            TestStatus::NotRun,
                            "BE-SYS-001",
                            "Worker rejected the request",
                            message,
                        ));
                    }
                    WorkerMessage::Result { result } => {
                        // Give the worker a moment to exit cleanly.
                        let t = Instant::now();
                        loop {
                            match child.try_wait() {
                                Ok(Some(_)) => break,
                                _ if t.elapsed() > Duration::from_secs(2) => {
                                    kill_tree(&mut child);
                                    break;
                                }
                                _ => std::thread::sleep(Duration::from_millis(10)),
                            }
                        }
                        break Ok(*result);
                    }
                }
            }
        }
    };
    let _ = child.try_wait();
    let stderr_text = stderr_handle.join().unwrap_or_default();

    let mut result = match outcome {
        Ok(mut r) => {
            // The supervisor, not the worker, is authoritative on provenance.
            if cfg.fixture_mode {
                r.provenance = Provenance::Fixture;
            }
            r
        }
        Err((status, code, title, detail)) => {
            let mut r = failed(test_id, status, code, title, detail, prov, started);
            if !stderr_text.trim().is_empty() {
                r.notes.push(format!(
                    "worker stderr: {}",
                    stderr_text.trim().chars().take(2000).collect::<String>()
                ));
            }
            r
        }
    };
    if let Some((code, message)) = safety_abort {
        result.status = TestStatus::Aborted;
        result.faults.insert(
            0,
            FaultRecord::new(&code, "Safety supervisor stopped the test", message),
        );
    } else if cancel.load(Ordering::SeqCst) && result.status == TestStatus::Pass {
        result.status = TestStatus::Aborted;
    }
    if cancel.load(Ordering::SeqCst) && !result.faults.iter().any(|f| f.code == "BE-SYS-003") {
        result.faults.push(FaultRecord::new(
            "BE-SYS-003",
            "Aborted by operator",
            "Stop requested",
        ));
    }
    if !warnings.is_empty() {
        if result.status == TestStatus::Pass {
            result.status = TestStatus::Warning;
        }
        result.faults.extend(warnings);
    }
    if let Some(m) = monitor.as_deref() {
        m.summarize(&mut result);
    }
    result
}
