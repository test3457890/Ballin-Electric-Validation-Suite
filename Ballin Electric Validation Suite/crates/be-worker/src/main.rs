//! bevalidate-worker: runs exactly one diagnostic test in an isolated
//! process. Protocol (see `be_core::ipc`): one `WorkerRequest` JSON line on
//! stdin; JSON-line `WorkerMessage`s on stdout. Any later stdin line, or
//! stdin EOF (supervisor gone), requests cancellation.

use be_core::ipc::{MAX_LINE_BYTES, PROTOCOL_VERSION, WorkerMessage, WorkerRequest};
use be_core::model::{EventLevel, Provenance};
use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn send(out: &Mutex<std::io::Stdout>, msg: &WorkerMessage) {
    let mut line = msg.to_line();
    if line.len() > MAX_LINE_BYTES {
        // Never violate the bounded protocol; truncate oversized logs.
        if let WorkerMessage::Log {
            level,
            code,
            message,
        } = msg
        {
            let cut: String = message.chars().take(4096).collect();
            line = WorkerMessage::Log {
                level: *level,
                code: code.clone(),
                message: format!("{cut}… [truncated]"),
            }
            .to_line();
        }
    }
    let mut o = out.lock().unwrap();
    let _ = o.write_all(line.as_bytes());
    let _ = o.flush();
}

fn main() {
    #[cfg(target_os = "linux")]
    // SAFETY: prctl(PR_SET_PDEATHSIG) only sets a flag on this process so the
    // kernel kills it if the supervisor dies; no memory is passed.
    unsafe {
        libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
    }
    let stdout = Arc::new(Mutex::new(std::io::stdout()));
    let stdin = std::io::stdin();
    let mut first = String::new();
    if stdin.lock().read_line(&mut first).is_err() || first.trim().is_empty() {
        send(
            &stdout,
            &WorkerMessage::Error {
                message: "no request received".into(),
            },
        );
        std::process::exit(4);
    }
    let req: WorkerRequest = match serde_json::from_str(first.trim()) {
        Ok(r) => r,
        Err(e) => {
            send(
                &stdout,
                &WorkerMessage::Error {
                    message: format!("invalid request: {e}"),
                },
            );
            std::process::exit(4);
        }
    };
    if req.protocol != PROTOCOL_VERSION {
        send(
            &stdout,
            &WorkerMessage::Error {
                message: format!(
                    "protocol {} unsupported (worker speaks {PROTOCOL_VERSION})",
                    req.protocol
                ),
            },
        );
        std::process::exit(4);
    }
    send(
        &stdout,
        &WorkerMessage::Hello {
            protocol: PROTOCOL_VERSION,
            pid: std::process::id(),
            engine_version: be_core::ENGINE_VERSION.into(),
        },
    );

    let Some(test) = be_diagnostics::find(&req.test_id, req.fixture_mode) else {
        send(
            &stdout,
            &WorkerMessage::Error {
                message: format!("unknown test id {}", req.test_id),
            },
        );
        std::process::exit(4);
    };

    let cancel = Arc::new(AtomicBool::new(false));
    {
        let cancel = cancel.clone();
        std::thread::spawn(move || {
            let mut l = String::new();
            // Any line or EOF means cancel.
            let _ = std::io::stdin().lock().read_line(&mut l);
            cancel.store(true, Ordering::SeqCst);
        });
    }
    let progress_state: Arc<Mutex<(Option<f64>, String)>> =
        Arc::new(Mutex::new((None, "starting".into())));
    let done = Arc::new(AtomicBool::new(false));
    {
        let (stdout, ps, done) = (stdout.clone(), progress_state.clone(), done.clone());
        std::thread::spawn(move || {
            while !done.load(Ordering::SeqCst) {
                let (p, phase) = ps.lock().unwrap().clone();
                send(&stdout, &WorkerMessage::Heartbeat { progress: p, phase });
                std::thread::sleep(Duration::from_millis(250));
            }
        });
    }
    let platform = be_platform::native();
    let params = if req.params.is_null() {
        test.descriptor().default_params
    } else {
        merge(test.descriptor().default_params, req.params.clone())
    };
    let ps2 = progress_state.clone();
    let progress = move |p: Option<f64>, phase: &str| {
        *ps2.lock().unwrap() = (p.map(|x| x.clamp(0.0, 1.0)), phase.to_string());
    };
    let so = stdout.clone();
    let log = move |level: EventLevel, code: Option<&str>, message: &str| {
        send(
            &so,
            &WorkerMessage::Log {
                level,
                code: code.map(String::from),
                message: message.to_string(),
            },
        );
    };
    let ctx = be_diagnostics::TestContext {
        params: &params,
        cancel: &cancel,
        progress: &progress,
        log: &log,
        provenance: if req.fixture_mode {
            Provenance::Fixture
        } else {
            Provenance::Live
        },
        platform: platform.as_ref(),
        high_load_authorized: req.high_load_authorized,
    };
    let result = be_diagnostics::execute(test.as_ref(), &ctx);
    done.store(true, Ordering::SeqCst);
    send(
        &stdout,
        &WorkerMessage::Result {
            result: Box::new(result),
        },
    );
}

/// Shallow merge of profile parameters over descriptor defaults.
fn merge(mut base: serde_json::Value, over: serde_json::Value) -> serde_json::Value {
    if let (Some(b), serde_json::Value::Object(o)) = (base.as_object_mut(), over.clone()) {
        for (k, v) in o {
            b.insert(k, v);
        }
        base
    } else {
        over
    }
}
