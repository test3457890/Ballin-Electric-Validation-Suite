# Architecture and decision record

Status: implemented for Milestones 0–2, with partial work toward Milestones 3–5 (see [Development Status](Development%20Status.md)). Last updated 2026-11-25.

## Layers and crates

```text
be-gui (FLTK-rs)      be-cli (bevalidate)                  front ends: no test logic
          \              /
           be-engine                                       profile runner, release review, capability registry
          /    |     \       \
 be-safety  be-storage  be-reporting  be-diagnostics(meta) supervision / persistence / reports
     |            \         /              |
     |             be-core  ---------------+               contracts: status, results, codes, profiles, policy, IPC
     |                                     |
 spawns ──────────► be-worker (bevalidate-worker) ──► be-diagnostics ──► be-platform
                    isolated process, one test         test modules       Linux sysfs/procfs, Windows CIM, NVML
```

| Crate | Responsibility | Depends on |
| --- | --- | --- |
| `be-core` | Test status semantics, typed results and measurements, fault-code catalog, versioned profiles, release policy, worker IPC protocol, CLI exit codes, UTC time | serde, thiserror |
| `be-platform` | Read-only inventory and sensor sampling. `LinuxProvider` (configurable root for fixtures), `windows_cim` (CIM via PowerShell JSON), runtime-loaded NVML | be-core, nvml-wrapper (optional) |
| `be-diagnostics` | Test modules with descriptors (ID, version, load class, privilege, limitations, evidence) | be-core, be-platform, sha2, libc |
| `be-worker` | `bevalidate-worker` executable: runs exactly one test per process | be-diagnostics |
| `be-safety` | Supervisor (spawn, bounded IPC, heartbeat, timeout, cancel, kill), safety policy and preflight, sensor watchdog | be-core, libc |
| `be-storage` | SQLite schema, migrations, repositories, audit log | be-core, rusqlite (bundled SQLite) |
| `be-reporting` | JSON, CSV and printable HTML reports, internal vs customer redaction | be-core, be-storage |
| `be-engine` | Shared application services for GUI and CLI | all of the above |
| `be-cli` | `bevalidate` command line for factory automation | be-engine |
| `be-gui` | `bevalidate-gui` FLTK-rs desktop interface | be-engine |

No cyclic dependencies. The production path never reaches fixture code: fixture tests are only registered when a worker is started with `fixture_mode=true`, and every fixture result is labeled `Provenance::Fixture`. The supervisor, not the worker, decides provenance. Release policy rejects fixture results.

## Decisions

| # | Decision | Rationale | Status |
| --- | --- | --- | --- |
| ADR-1 | Rust edition 2024, toolchain pinned to 1.97.0 (`rust-toolchain.toml`), `Cargo.lock` checked in | Reproducible builds; user requirement | Accepted |
| ADR-2 | FLTK-rs 1.5 (FLTK 1.4, built from source by `fltk-sys`) for the GUI | Approved classic desktop look, compiled, no webview | Accepted; accessibility limitation documented in [Accessibility](Accessibility.md) |
| ADR-3 | Each test runs in a separate worker process with JSON-lines IPC (protocol v1, 4 MiB line bound, bounded channel) | Crash/hang isolation, forced termination, UI responsiveness | Accepted |
| ADR-4 | Linux workers run in their own process group (SIGKILL to the group on abort) plus `PR_SET_PDEATHSIG`; stdin EOF means cancel | Clean termination including orphan protection | Accepted. Windows Job Objects are planned (current workers spawn no children) |
| ADR-5 | High-load tests require operator authorization AND an approved, device-specific hardware safety profile; no universal limits are built in | Spec §8: no guessed thresholds | Accepted. Only an unapproved example profile ships |
| ADR-6 | Release requires a release-qualifying profile, all required tests PASS (or permitted waivers on non-failed waivable steps), live provenance, signed manual checks, and a separate authorized approval | Spec §10 | Accepted |
| ADR-7 | Results, baselines and the audit log are immutable (SQLite triggers); rework creates a new linked run | Auditability | Accepted |
| ADR-8 | Windows inventory uses documented CIM classes through in-box PowerShell (`Get-CimInstance ... | ConvertTo-Json`) instead of COM bindings | Avoids large unsafe FFI in the first release; parsing is fixture-tested | Accepted provisionally; native `windows`/`wmi` backend may replace it |
| ADR-9 | SMART/NVMe health via an installed `smartctl` (separate process, JSON output), never bundled | smartmontools is GPL-2.0; invocation of a separately installed tool does not link it | Accepted; see Third Party Notices |
| ADR-10 | GPU compute, VRAM and graphics workloads are not yet implemented and report UNSUPPORTED (BE-GPU-001) | No verified backend yet; never fake results | Open: Vulkan compute via `ash` is the leading candidate (Milestone 3) |
| ADR-11 | No native PDF generation; HTML reports are print-ready | Avoid unreviewed PDF library licenses | Open |
| ADR-12 | Unsafe code is limited to small documented modules: volatile memory access, thread affinity, statvfs, posix_fadvise, process-group kill, prctl, geteuid, SIGINT handler | Spec §6 | Accepted |

## Test module contract

Every module declares: ID, version, name, subsystem, description, load class (`read_only`, `low`, `moderate`, `high`), privilege, destructive flag (always false in this release), typical duration, cancel behavior, evidence, limitations, manual steps and default parameters. Results carry status, UTC start time, monotonic duration, coverage text, measurements (value or *unavailable*, never zero-filled), faults with catalog codes, notes, engine version and provenance.

Statuses are distinct: PASS, FAIL, WARNING, NOT RUN, UNSUPPORTED, SKIPPED, ABORTED, INCONCLUSIVE, RUNNING, PENDING. Only PASS satisfies a required step, plus WARNING if the profile explicitly allows it.

## Supervision sequence

1. The engine checks prerequisites. A step whose prerequisite did not pass is recorded as NOT RUN with BE-SYS-004.
2. Preflight (`be_safety::policy::preflight`) runs. Destructive tests are refused. High-load tests need authorization, an approved profile, required sensors present and reporting, and readings below their warning limits.
3. The supervisor spawns `bevalidate-worker` and sends `WorkerRequest`. The worker answers with `Hello`, sends a `Heartbeat` every 250 ms with measured progress where available, then `Log` lines and finally `Result`.
4. The supervisor enforces the overall timeout (BE-SYS-002) and heartbeat silence (BE-SYS-002). A crash or exit without a result is BE-SYS-001. A protocol violation is INCONCLUSIVE. An operator Stop sends a cancel line, then force-kills the process group after a grace period (BE-SYS-003).
5. For high-load steps the sensor watchdog samples the platform at the profile interval. Reaching an abort limit gives BE-THERM-002; a required sensor missing for N samples gives BE-SNS-001; a warning limit adds BE-THERM-001. Min, max and average telemetry are added to the result.
6. The result is persisted immutably. At run end, release policy is evaluated and a disposition recorded. Disposition is never set to `Released` automatically.

## Data model (SQLite schema v1)

`units`, `runs` (profile JSON, inventory snapshot, engine version, station, operator, authorization, fixture flag, owner PID), `results` (immutable), `events`, `manual_checks`, `waivers`, `dispositions`, `service_records`, `baselines` (immutable), `audit_log` (append-only). Migrations use `PRAGMA user_version`. A database newer than the application is refused. WAL journal with `synchronous=FULL`. `quick_check` runs on open. The directory is created 0700 and the file 0600 on Unix. Runs left `running` by a dead process are marked `interrupted` with BE-PWR-001 at the next start.

## Threading in the GUI

The FLTK main thread owns all widgets. Inventory scans and profile runs execute on background threads with their own SQLite connection and post messages through `fltk::app::channel`. The worker runs in a separate process, so the UI stays responsive during tests. Sensor sampling for the Sensors tab runs on the main thread once per second; it is file reads on Linux and NVML only on Windows.
