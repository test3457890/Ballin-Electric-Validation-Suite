# Ballin Electric System Validation Suite

## Claude Code development brief and product requirements

Version: 0.2 (Rust-first revision) | Status: Proposed requirements | Project: BE Validation

This document is the implementation brief for Claude Code. It is a planning and development specification, not evidence of completed software, validated hardware, certified safety, or commercial license clearance.

## 1. Instructions to Claude Code

You are the lead software engineer responsible for building a professional OEM computer manufacturing, hardware diagnostic, burn-in, stability testing and quality assurance application for Ballin Electric. Build actual working engineering software, not a dashboard demonstration, a website, or a collection of mock tests.

Read this entire brief before editing anything. Inspect the existing workspace and Git repository. Preserve working code and existing user changes. If the workspace is empty, create a maintainable project structure. Write a short implementation plan, architecture decision record, capability matrix, and milestone checklist, then begin implementing the first milestone. Do not stop after producing a plan, visual shell or README. Follow the milestone order, run tests as you go, fix regressions, and document what actually works.

If a dependency, platform, device, driver, commercial license or test instrument is unavailable, implement an honest capability check and mark the feature as unsupported or blocked. Never substitute a fake passing result. Keep building independent functionality where possible. Do not install privileged drivers, change firmware, modify power limits, erase disks, start intensive physical hardware tests, publish code, or create external services without appropriate authorization. Do not automatically run a stress test on the development machine. Use mocks only in explicitly labeled development and automated test fixtures. The production application must not silently use mock data.

The user has approved the traditional industrial desktop design direction described below. Do not replace it with a trendy SaaS-style design. No major visual redesign is required before implementing the initial shell.

At the end of each milestone, record completed work, commands executed, actual results, known issues, unsupported hardware, licensing blockers and the next milestone. Keep the repository usable and runnable. Do not claim a test passed if it was not executed. Do not claim support for actual BE-01 hardware until it has been tested on that hardware.

## 2. Product purpose

Ballin Electric is developing a boutique portable desktop-class workstation called the BE-01. The software should eventually be useful on other workstations, desktops, laptops, servers, repair benches and production lines as well.

BE Validation combines the following functions into one modular application:

- OEM manufacturing diagnostics and assembly verification.
- Automated factory acceptance testing and quality control.
- CPU, GPU, VRAM, memory, storage and system stability testing.
- Extended burn-in and endurance testing.
- Hardware identification, health monitoring and fault isolation.
- Performance benchmarking, treated as secondary to reliability.
- Customer troubleshooting and repair verification.
- Preboot diagnostics for computers that cannot start their installed operating system.
- Repeatable, auditable reports and lifetime service history.

The objective is to identify actual errors, isolate likely defective components, record evidence, and prevent unvalidated systems from being released for shipment. A high benchmark score must never override a hardware fault.

### Target hardware context

The BE-01 is a proposed dual 18-inch 4K display clamshell workstation, with no main internal battery, external keyboard and mouse, desktop-class processor, two desktop GPUs, internal AC power supply, NVMe storage and 10 Gigabit Ethernet. The initial standard configuration targets a single 1,200 W SFX-L PSU and up to two 300 W GPUs, such as NVIDIA RTX PRO 6000 Blackwell Max-Q (96 GB each) or AMD Radeon AI PRO R9700 (32 GB each). The enclosure, cooling, PCIe risers, display interfaces and supported configurations remain subject to engineering validation.

Support separate GPUs by stable device identifiers and physical slot mapping. Combined physical VRAM does not mean a single contiguous memory pool. Do not assume specific PCIe widths, sensors, GPU APIs, display refresh rates or firmware features. Obtain these from inventory, supported device interfaces and the approved configuration manifest. No single BE-01 configuration is guaranteed to fit, operate safely or pass regulatory testing based on this software specification.

## 3. Visual identity (approved direction)

The program should feel like mature, traditional professional engineering software. It should look appropriate in a computer repair workshop, manufacturing test station, aerospace maintenance environment or research laboratory. Aim for a classic Windows desktop utility from the late 1990s through the 2010s, refined for modern displays and accessibility. Do not make it look deliberately broken or obsolete.

Design references (inspiration only, not assets or interfaces to copy):

- HWiNFO: dense sensor tables and hardware tree.
- CPU-Z: concise tabs, technical specifications and readable labels.
- Windows Computer Management / MMC / Event Viewer: split panes, navigation tree, event records and conventional menus.
- PassMark BurnInTest: practical test controls and results.
- Prime95: direct, utilitarian engineering tools and logs.
- PC-Doctor Factory: manufacturing diagnostics and workstations.
- Automotive diagnostic software such as Toyota Techstream: failure codes and guided component troubleshooting.

### Non-negotiable visual rules

Use FLTK-rs (the fltk crate) for a compiled cross-platform desktop interface, not a browser or webview. Use MenuBar, Tree, TableRow/Table, Tabs, Tile, Pack/Flex, dialogs, Progress and TextDisplay, with conventional toolbars and status bars assembled from simple widgets. Prefer traditional desktop controls and a restrained style layer. FLTK draws its own widgets; verify keyboard navigation, screen-reader accessibility and high-DPI behavior rather than claiming native OS accessibility automatically. If these requirements cannot be met, document the limitation and propose a Rust-compatible alternative before changing the UI toolkit. No React, Electron, Tailwind, webview shell, Material-style cards, oversized dashboard metrics, glassmorphism, gradient backgrounds, floating navigation, animations, rounded pill buttons, gaming RGB, chatbot-first layouts or large decorative headings. Avoid excessive empty space. No unnecessary splash screen.

Use a rectangular, information-dense layout with functional icons, small headings, grid lines, resizable panes, column sorting and conventional keyboard shortcuts. Use normal desktop menu actions and right-click context menus. Keep status text visible in addition to color. Provide accessible labels, focus indicators, high-DPI scaling, keyboard navigation and user-adjustable text size. Support a practical minimum window size around 1024 x 768 and scale properly on dual 4K displays.

### Default Industrial theme

The exact values below are proposed starting design tokens, not a requirement to override OS accessibility settings.

| Element | Value |
| --- | --- |
| Main chrome | #D5D9DC |
| Toolbar | #E2E5E7 |
| Title / header accent | #34434E |
| Content pane | #FFFFFF |
| Secondary pane | #F1F3F4 |
| Borders | #929CA4 |
| Text | #202020 |
| Selected row | #CBDCEE |
| Pass | #26752A |
| Warning | #946600 |
| Fail | #BD1818 |
| Running | #245CA2 |
| Pending / unknown | #666666 |
| Event log background | #181818 |

Use Segoe UI or Tahoma on Windows, a suitable native fallback on Linux, and Consolas or a system monospace font for logs. Use native DPI-aware sizing around 9 to 10 pt for tables and menus, with adjustable scaling. Typical toolbar icons should be 16 to 24 logical pixels. Minimal spacing, square corners, subtle one-pixel separators. Graphs should resemble scientific instrumentation: axes, units, grid lines, legends, selectable sensors and time ranges.

Offer two additional themes after the default is functional: Classic (Windows 2000/XP-style gray #D4D0C8 and navy #123D79), and Dark (charcoal #262A2D, panels #34393D and restrained status colors). Use the same layout in all three. Honor high-contrast and accessibility preferences.

### Branding

Application title: Ballin Electric System Validation Suite
Short name: BE Validation
Subtitle: Hardware Diagnostics & Manufacturing Quality Assurance
Use a small, polished stainless-steel sphere logo, consistent with the proposed physical bearing logo on the BE-01. A simple original monochrome or metallic sphere icon is sufficient. Do not use third-party corporate logos or imply affiliation with Dell, Lenovo, HP, NVIDIA or any other manufacturer. No oversized branding inside the workspace.

### Main window layout

The following wireframe is illustrative. The displayed test results are examples only and must not appear as real data in production.

```text
+----------------------------------------------------------------------------------+
| (o) Ballin Electric System Validation Suite                         [_] [ ] [X] |
+----------------------------------------------------------------------------------+
| File  System  Diagnostics  Tests  Manufacturing  Reports  Tools  View  Help     |
+----------------------------------------------------------------------------------+
| [Scan] [Run Tests] [Stop] [Results] [Save] [Export] [Print]                     |
+-------------------------+--------------------------------------------------------+
| SYSTEM NAVIGATOR        | System Validation                                      |
|                         | Serial: BE01-000001 | Factory profile 01              |
| v BE-01 Workstation     | [Overview] [Hardware] [Sensors] [Tests] [Errors]       |
|   > Processor           +-----------------------------+----------+---------------+
|   > System Memory       | Test module                 | Result   | Duration      |
|   > Motherboard         +-----------------------------+----------+---------------+
|   v Graphics Devices    | CPU Arithmetic              | PASS     | 00:10:00      |
|       GPU 0             | System Memory               | PASS     | 01:20:00      |
|       GPU 1             | GPU 0 VRAM                  | PASS     | 00:20:00      |
|   > Storage Devices     | GPU 1 VRAM                  | FAIL     | 00:17:35      |
|   > Cooling System      | Combined Load               | NOT RUN  |               |
|   > Power Delivery      | Final Inspection            | PENDING  |               |
|   > Displays            +-----------------------------+----------+---------------+
|   > Network Devices     | Selected test details / measured values / evidence     |
|   > External Ports      | BE-GPU-014: Memory mismatch                            |
|   > Firmware            | [View log] [Run targeted diagnostic]                    |
|   > Manufacturing       | Progress [======================          ]            |
|   > Service Records     | Shipment disposition: HOLD                             |
+-------------------------+--------------------------------------------------------+
| DIAGNOSTIC EVENT LOG                                            [Filter] [Clear] |
| 14:20:01 INFO   Hardware enumeration complete.                                    |
| 14:24:12 ERROR  BE-GPU-014: Memory mismatch detected.                             |
| 14:24:13 WARN   Combined load blocked pending fault review.                       |
+----------------------------------------------------------------------------------+
| Offline | Operator: Local | Privilege: Standard | 1 Error | v0.1.0              |
+----------------------------------------------------------------------------------+
```

Use a real menu system, dockable/resizable event log, expandable hardware tree, tabbed details, sortable test results and a status bar. Toolbars should be compact. The stop control must remain reachable while tests run. The UI must remain responsive even during intensive tests.

## 4. Application modules and operating modes

All editions should share one diagnostic engine, data model and report format. Avoid creating five unrelated applications.

| Module | Responsibility |
| --- | --- |
| System Information | Device inventory and configuration comparison |
| Diagnostics | Component self-tests, hardware errors and fault isolation |
| Stress Tests | CPU, GPU, VRAM, RAM and combined workloads |
| Stability | Error-detecting workloads, load changes, cold and warm checks |
| Burn-In | Long-duration scripted test sequences |
| Benchmarks | Reproducible measurements and baseline comparison |
| Monitoring | Live temperatures, clocks, fan speeds, power, errors and time series |
| Manufacturing | Unit records, assembly checkpoints and test scheduling |
| Quality Assurance | Acceptance criteria, manual inspection and release disposition |
| Troubleshooting | Evidence-driven fault trees and recommended follow-up tests |
| Service Records | Repairs, part replacement, upgrades and retesting |
| Security | Firmware information, Secure Boot, TPM and OS integrity checks |
| Reports | Printable certificates, technical reports and machine-readable evidence |
| Test Designer | Versioned custom test profiles, limits, ordering and prerequisites |
| Preboot | Separate bootable environment using the shared formats |

Proposed future editions: Factory, Engineering, Service, Customer, and Bootable Diagnostics. For the initial version implement modes in the same application, with distinct permissions where needed. Customer mode must not expose unrestricted factory controls or destructive operations.

## 5. Existing software references and licensing

Use these products as behavioral references. Do not copy source, proprietary assets, test algorithms or redistribution-restricted binaries without permission. Review the license for every dependency, tool, SDK and driver; record version, source, license, commercial use, redistribution, integration and attribution conditions. A public download does not imply permission to bundle or resell it.

| Reference | Capability to learn from |
| --- | --- |
| Prime95 | CPU torture tests and verified arithmetic errors |
| FurMark | Sustained graphics load and heat generation |
| OCCT | Error-aware stability and combined system tests |
| PC-Doctor Factory | OEM production validation, diagnostics and records |
| PassMark BurnInTest | Scripted multi-component burn-in |
| MemTest86 / Memtest86+ | Preboot memory checks and address patterns |
| HWiNFO / AIDA64 | Hardware identification and telemetry |
| CPU-Z / GPU-Z | Detailed component information |
| CrystalDiskInfo / CrystalDiskMark | Storage health and performance |
| Cinebench / 3DMark | Reproducible rendering and graphics workloads |
| y-cruncher / TestMem5 | Computation and memory stability |
| NVIDIA DCGM | GPU diagnostics on explicitly supported devices |
| smartmontools / nvme-cli | Storage health and self-tests |
| stress-ng / fio / iperf3 | Linux stress, storage and network workloads |
| Dell ePSA / Lenovo UEFI Diagnostics / HP PC Hardware Diagnostics | Offline hardware tests and actionable failure codes |

Start with original basic test kernels and supported OS/vendor APIs. Adapters for installed third-party tools are optional and must not bypass license restrictions. Do not package or invoke proprietary tools commercially unless their terms allow the intended use. Open-source licenses also have obligations. Track external dependencies in THIRD PARTY NOTICES and a dependency manifest/SBOM. Verify FLTK-rs and underlying FLTK licenses, including the LGPL static-linking exception and applicable notice and modification obligations; review all crate, driver, SDK and external tool licenses for commercial use and distribution. Do not assume a wrapper license grants rights to its underlying vendor SDK.

## 6. Technical architecture

Use stable Rust (edition 2024) for all first-party application code, including the FLTK-rs desktop interface, CLI, test orchestrator, diagnostic modules and isolated workers. Use a Cargo workspace, checked-in Cargo.lock, pinned rust-toolchain.toml and reproducible documented builds. Use cargo fmt --all -- --check, cargo clippy --workspace --all-targets and cargo test --workspace for safe automated verification. Use SQLite via rusqlite for local records. Consider PostgreSQL only for a later optional multi-station factory server. Keep core operation fully offline. Python is optional for development fixtures only; no Python runtime required for the shipped app.

Rust-first does not mean that all dependencies are implemented in Rust: FLTK uses C/C++ internally and source builds may require CMake and a C++ compiler; GPU kernels or vendor SDKs may need CUDA C, HIP, SPIR-V or native libraries. Keep those exceptions narrowly scoped, behind audited Rust adapters, and document them. Do not replace the Rust core with a C++ application. Use safe Rust by default; isolate and justify unsafe FFI, hardware access and intrinsics in small reviewed modules. Do not create a custom kernel driver for initial releases.

Recommended crate candidates, subject to actual API compatibility and license review:

| Purpose | Candidate |
| --- | --- |
| GUI | fltk |
| Inventory | sysinfo plus native platform providers |
| Windows | windows, wmi; documented Event Log APIs |
| Linux | sysfs/hwmon via std, rustix or nix where needed |
| NVIDIA | nvml-wrapper; runtime capability detection |
| GPU compute | ash (Vulkan), optional cudarc or audited CUDA FFI, supported HIP adapter |
| CLI | clap |
| Data models and profiles | serde, serde_json, toml |
| Database | rusqlite |
| Logging | tracing, tracing-subscriber |
| Errors | thiserror; anyhow at executable boundaries |
| Worker messages | bounded channels and versioned IPC, using std or crossbeam-channel as appropriate |
| CPU workers | std threads and feature-detected SIMD; rayon only where controlled concurrency permits |
| Test fixtures | tempfile, proptest where useful |

Pin compatible versions only after verifying actual APIs. Run dependency license and advisory checks (for example cargo-deny and cargo-audit); generate an SBOM and record exceptions. Do not use a generic inventory library as evidence that all sensors or hardware fault diagnostics are supported.

Proposed layers:

```text
FLTK-rs GUI / Rust CLI / future bootable front end
                  |
Application services / profile runner / permissions
                  |
Test scheduler / safety supervisor / result evaluator
                  |
Capability registry / hardware inventory / fault analysis
                  |
OS backends / vendor APIs / isolated test workers / external instruments
                  |
SQLite repository / structured logs / reports / signed release records
```

- Separate UI, device providers, workload workers, test registry, policy engine, persistence and report generation. Use typed interfaces and structured data, not parsing screen text.
- Long-running workloads should execute in isolated worker processes where practical; use bounded IPC, cancellation, timeouts, heartbeats, exit-code handling and resource cleanup. The UI must not block. The supervisor must terminate process trees on abort (Windows Job Objects and appropriate Linux process groups or cgroups, as supported); preserve logs on failure. A separate watchdog and hardware safeguards are necessary for serious unattended testing. Software cannot guarantee recovery from a kernel or hardware hang.
- Windows: use documented WMI/CIM, SetupAPI, DXGI, native storage APIs, Windows Event Log/WHEA, and supported vendor APIs. Linux: use sysfs, hwmon, procfs, udev, PCI/device interfaces, system logs and supported vendor APIs. Respect OS permissions. Do not require administrator access for read-only functionality unless genuinely needed.
- NVIDIA: NVML for supported monitoring, CUDA for original verified computation, optional nvidia-smi integration, DCGM only if the specific GPU and software combination supports it. AMD: documented AMD SMI/HIP/ROCm interfaces where supported. Do not assume consumer and workstation cards expose all metrics or diagnostics. Use Vulkan compute/graphics or other supported backends as appropriate. Add DirectX/OpenGL later if useful.
- Use separate executable workers for privileged or high-risk operations; authorize specific operations rather than running the entire UI as administrator. No unsigned kernel drivers, undocumented hardware-register writes, automatic overclocking, voltage changes, firmware flashing or security setting changes.
- Use stable error codes, versioned schemas, device IDs, UTC timestamps and monotonic durations. Preserve raw readings, units and sampling intervals. Track data quality and unavailable metrics. A missing sensor is not zero. Store test engine and configuration versions with each result.
- Use prepared database queries, restricted file permissions, atomic writes, migrations and recovery checks. Do not store user secrets or personal documents in diagnostic logs.
- Add a command-line interface for automated factory operation. GUI and CLI must execute the same test engine and acceptance policy.

Suggested source layout (adapt if an existing repository has conventions):

```text
Cargo.toml              workspace manifest
Cargo.lock              locked dependency versions
rust-toolchain.toml     pinned stable toolchain
README.md
docs/                   architecture, build, safety, profiles, capability matrix
crates/be-gui/          FLTK-rs desktop windows and views
crates/be-cli/          command-line entry point
crates/be-core/         contracts, result models, scheduler and policy
crates/be-platform/     Windows and Linux providers
crates/be-diagnostics/  test modules and vendor adapters
crates/be-worker/       isolated workload executables
crates/be-safety/       supervisor, guardrails and watchdog interfaces
crates/be-storage/      SQLite schema, repositories and migrations
crates/be-reporting/    HTML, PDF, JSON, CSV and print support
resources/              original icons, themes and translation strings
profiles/               versioned test and hardware manifests
tests/                  end-to-end fixtures; crate-specific integration tests in each crate
packaging/              Windows and Linux distribution recipes
```

Keep dependencies directional and avoid cyclic crate relationships. Start with a small number of crates if necessary and split as real implementation boundaries emerge. The listing is conceptual; Cargo package names may use hyphens, while Rust module identifiers follow Rust syntax. Use ordinary human-readable product names in the UI.


## 7. Required diagnostic coverage

| Subsystem | Required tests / evidence | Limitations |
| --- | --- | --- |
| CPU | Verified integer/floating-point computations, cache and per-core workloads, supported vector instruction sets, error counters and thermal/clock readings | Feature detect instructions; do not force unsupported AVX variants |
| GPU compute | Deterministic compute kernels, known-answer or CPU reference comparisons, separate runs per GPU and simultaneous runs | Vendor backend support varies; check allocation and driver status |
| GPU graphics | Rendering stress, repeated scenes, deterministic verification where possible, device loss and driver resets | A FurMark-style workload alone does not establish stability |
| VRAM | Pattern writes/reads in owned allocations, mismatch detection, supported ECC counters, independent device coverage | Never claim all physical VRAM tested if OS reservations or driver limit access |
| System RAM | Address/data pattern testing in allocated buffers, ECC/OS error monitoring, longer preboot testing | OS-based tests cannot cover all reserved physical memory |
| NVMe / SSD | SMART health, error counters, supported device self-tests, thermal trends, optional integrity/throughput tests on dedicated scratch files | Read-only default; no raw writes or destructive commands on customer disks |
| Motherboard | Inventory, firmware, chipset/device enumeration, error reporting | Cannot diagnose every board-level electrical fault from software |
| PCIe | Negotiated link speed/width, expected link comparison, transfer tests, WHEA/AER errors when available | Link downshift at idle may be normal; observe under controlled load |
| PSU / power | Combined CPU/GPU workload, estimated component draw, resets, external wall power where instrumented | Cannot certify ripple, grounding, transient response or mains safety using software telemetry |
| Cooling | Temperature, clock throttling, supported tachometers, thermal trends, cooldown | Fan speed and hotspot readings are not available on every system |
| Displays | EDID/mode detection, test patterns, resolution/refresh checks, independent output verification | Pixel defects, calibrated color and luminance require operator or instruments |
| Networking | Link speed, loopback or authorized iperf3 peer, loss and throughput | No external or unauthorized traffic by default; 10GbE test requires capable peer |
| USB | Enumerate controllers/devices, test fixtures, scratch-file transfer integrity | Physical connector coverage requires a known test device and operator mapping |
| Wireless | Optional Wi-Fi/Bluetooth device detection, authorized connectivity/pairing | Not applicable when hardware absent |
| Audio | Endpoint enumeration, guided audio loopback where fixture available | Manual listening or calibrated hardware may be needed |
| Firmware / security | UEFI, BIOS version, TPM, Secure Boot, approved configuration comparison | Read-only; never automatically change firmware settings |
| OS / drivers | Driver inventory, hardware events, crash evidence, system integrity checks | Do not repair or change the OS without separate authorization |
| Professional workloads | Known-answer AI/tensor tests, local inference, multi-GPU compute, rendering samples | Use redistributable datasets/models and verified reference outputs; no customer data |
| System reliability | Controlled restarts/cold boots, idle/load transitions, thermal soak and recovery | Restart only in explicitly authorized factory runs with resumable state |

Each test module must declare ID, version, device target, capabilities, required privileges, destructive flag, expected resource use, duration, cancel behavior, sensors, evidence, thresholds and required manual steps. Results must include coverage, actual duration, measurements, error details and result provenance. Implement Pass, Fail, Warning, Not Run, Unsupported, Skipped, Aborted, Inconclusive and Running as distinct statuses. Never convert unsupported/skipped/aborted to pass. A benchmark can report a score but must not hide errors.

## 8. Test profiles, sequencing and safety

Proposed starting profiles (durations are planning ranges, not validated production limits):

| Profile | Duration | Purpose |
| --- | --- | --- |
| Quick Diagnostics | 5 to 15 minutes | Inventory, health and selected low-impact checks |
| Standard Validation | 30 to 60 minutes | Broad functional coverage |
| Extended Stability | 2 to 4 hours | Isolated and combined error-aware tests |
| Factory Burn-In | 8 to 24 hours | Versioned acceptance suite with manual checkpoints |
| Engineering Endurance | 48 to 72 hours | Prototype reliability, only after safe supervised validation |

Suggested factory sequence: inventory and manifest verification, preflight safety check, idle baseline, RAM and storage tests, isolated CPU and individual GPU tests, both GPUs, progressively ramped combined workloads, representative AI/rendering workloads, cooldown, external/physical inspection, final report and approval. Do not automatically start combined load if a prerequisite failed. Permit a qualified operator to abort any run. Run tests in parallel only when an approved profile and resource budget allow it.

### Safety requirements

- Read-only inventory and diagnostics by default. Starting sustained workloads requires explicit user action, a visible load warning and an applicable safe profile. No stress on application startup.
- Use device-specific documented limits and an approved engineering safety envelope. Do not guess universal temperature or power thresholds. For unrecognized devices or missing critical sensors, do not launch high-risk profiles until safe limits are established. Use conservative approved limits, not the highest allowed junction temperature as the default target.
- Ramp load gradually, monitor telemetry independently, check sensor freshness, and immediately abort on critical temperature, sensor loss, relevant fan failure, severe errors or operator stop. Warn or abort on abnormal throttling according to profile. Ensure cooldown checks after cancellation. Avoid unsafe retry loops.
- Reserve system memory and VRAM; handle OOM gracefully. Bound storage capacity, writes and time. Use dedicated temporary test files with safe paths, free-space checks, and cleanup. No raw disk writes, format, erase, secure erase or destructive tests in the initial release.
- Record unexpected shutdowns and incomplete runs. Resume only from a safe verified state and never treat an interrupted test as passed. No automatic repeated reboot cycles without approved factory profile and operator consent.
- External electrical measurement, calibrated thermal measurement, mechanical tests, and regulatory certification are independent of this program. Do not market BE Validation as a substitute for them.

## 9. Intelligent fault isolation

Use deterministic, explainable rules first. A future AI assistant may summarize evidence but must never override safety or final acceptance criteria. A failure does not automatically establish the root cause. Show confirmed observations, hypotheses, confidence/evidence, suggested next safe tests and escalation instructions separately.

| Observation | Possible causes | Follow-up |
| --- | --- | --- |
| CPU arithmetic mismatch | CPU, RAM, memory controller, unstable settings | Repeat verified workload, run memory tests, inspect firmware and error logs |
| GPU VRAM mismatch | VRAM, GPU, driver or unstable power | Repeat independent GPU test, review ECC, compare known-good configuration |
| GPU device disappears | PCIe riser, power, GPU, driver | Inspect link/errors, isolate each GPU, technician checks connections with power removed |
| Individual tests pass but combined load fails | Power delivery, shared thermal limits, motherboard | Hold unit, review logs and temperatures, use qualified external power testing |
| SSD drops under load | SSD controller, thermals, motherboard | Review SMART/error logs, read-only check, inspect mounting/cooling |
| Display flickers | Cable, panel/controller, GPU driver | Known test pattern, alternate authorized output, manual inspection |

Use stable codes such as BE-CPU-001, BE-GPU-014, BE-MEM-001, BE-PCIE-003, BE-THERM-002, BE-PWR-001. Maintain a versioned catalog documenting meaning and diagnostic guidance. Never label an unconfirmed hypothesis as a diagnosed failed component. Example codes are proposed, not existing vendor codes.

## 10. OEM manufacturing, quality assurance and service

A workstation record should include internal serial, chassis revision, order reference, approved BOM/configuration, CPU/GPU/RAM/storage serials where available, firmware and drivers, assembly technician, date, test station, approved profile version, environmental conditions where measured, full test run history, event evidence, physical inspection, repairs/rework, reviewer and final disposition. Preserve previous baselines after upgrades rather than overwriting them.

Manual factory inspection checklist must cover chassis finish, assembly fasteners, GPU retention, wiring and connector seating, unobstructed ventilation, hinges, kickstand stability, screen condition, port checks, accessories and packaging. Electrical safety results may be recorded from approved external procedures or lab reports. The application must identify manual or instrument-based tests explicitly.

Use release states: Awaiting Assembly, Awaiting Test, Testing, Hold, Rework, Awaiting Review, Released. Required tests must pass and required manual checks must be signed off. A failed, aborted, unsupported or missing required test blocks release. Optional unsupported tests may be documented as not applicable according to approved policy. Waivers must be explicitly permitted, recorded with reason and reviewer identity, and never override critical safety requirements. Rework requires a new linked run, not editing old results. Release approval should be a deliberate authorized action. Do not claim enforcement against real shipping systems until a real integration exists.

For a later multi-station factory deployment, add operator/reviewer roles, separate release approval, test queues, barcode scanning, shared records and audit history. Avoid a cloud dependency. For low-volume initial production, a local station and exportable records are sufficient. Protect sensitive serials and customer data, and allow redacted customer reports. Use signed or tamper-evident reports in a later milestone with documented key management; hashes alone do not prove authenticity.

## 11. Reporting and command line

Provide printable HTML/PDF factory acceptance reports and detailed service reports, plus JSON and CSV export. Include system identity, profile/engine versions, timestamp, results, units, test coverage, thresholds, unavailable sensors, raw evidence references, failures, rework history, manual checks and final disposition. Customer reports should omit unnecessary internal or personal information. Label incomplete, simulated, engineering-only or unvalidated reports clearly. Factory acceptance is not UL, FCC or other independent certification.

Proposed CLI interface (implement and document exact options):

```text
bevalidate inventory --format json
bevalidate capabilities --format json
bevalidate tests list
bevalidate profile validate profiles/quick.json
bevalidate run --profile quick --output ./results
bevalidate report --run-id <id> --format pdf
```

Actual high-load profiles must require explicit authorization options and safety checks. Use documented exit codes for pass, fail, incomplete/unsupported, aborted, invalid input and internal error. A successfully generated report is not equivalent to a passing test. Keep headless automation consistent with GUI behavior.

## 12. Development milestones and acceptance gates

### Milestone 0: Foundation and approved interface

Create the Rust Cargo workspace, FLTK-rs GUI and CLI targets, architecture docs, dependency/license inventory, initial SQLite schema, test contracts and capability registry. Implement the traditional desktop shell with real menu actions, tree navigation, resizable split panes, test table, details, event log and status bar. Use an explicit demo fixture only for design screenshots. Production mode must start with real inventory or an honest unavailable state. Verify window resizing, high-DPI handling and keyboard access. Build both executable targets with Cargo, run formatting, lint and fixture tests, and document GUI accessibility limitations and native build dependencies.

### Milestone 1: Useful real diagnostics

Implement native inventory on at least the development platform, CPU/RAM/storage enumeration, safe OS event collection, selected supported sensor reads, SQLite persistence, JSON/CSV export and CLI. Add second-platform interfaces and document unsupported paths. Add at least one genuine low-impact diagnostic with known-answer verification. Tests and documentation must distinguish fixture-only tests from live hardware checks.

### Milestone 2: Stability engine and supervision

Implement isolated CPU verified computation workers, allocated-buffer RAM tests, scratch-file integrity checks, scheduler, cancellation, timeouts, safety policy, sensor freshness checks and failure preservation. Add unit/integration tests for mismatch, worker crash, timeout, stop, missing sensor and simulated overtemperature. No physical stress test without user authorization. Validate controlled live workloads on known hardware only after safety review.

### Milestone 3: GPU and system validation

Implement supported GPU inventory and telemetry, deterministic compute kernels, allocated VRAM checks, independent and simultaneous device selection, PCIe health, graphics workload backend, progressively ramped combined tests and instrument interfaces. Document hardware/driver support matrix and known limitations. Test on actual supported single and dual-GPU machines before claiming support. Add real measurements for representative AI/rendering workloads when appropriate and licensed.

### Milestone 4: Manufacturing and service workflow

Implement serial/BOM records, baseline comparison, versioned profiles, checklist, fault catalog, rework history, release policy, HTML/PDF reports and audit trail. Test that an incomplete or failed required test blocks release. Validate data recovery, migration, export and separation of customer/internal reports.

### Milestone 5: Cross-platform packaging and customer diagnostics

Complete Windows/Linux backends as hardware and drivers permit. Add installer packaging, documented dependencies, safe customer mode, optional Classic and Dark themes, accessibility testing, support documentation and explicit user-initiated support bundle export. Plan code signing and secure distribution; do not claim signed releases without actual keys and signing.

### Milestone 6: Preboot and advanced factory operation

Prototype a bootable Linux USB environment and evaluate suitable memory diagnostics under applicable licenses. Reuse profile and result schemas. Test Secure Boot compatibility, UEFI, driver availability and offline report export; do not require customers to disable security features by default. Later evaluate native UEFI diagnostics, authorized OEM firmware integration, multi-station server, barcode scanners, external instruments, role-based audit and controlled shipping integration.

## 13. Verification and delivery expectations

- Build and run with documented commands. Include dependencies and versions, clean build instructions and platform limitations. Keep installation steps reproducible.
- Unit-test result evaluation, unit conversion, threshold behavior, capability detection, profile parsing, release logic and database migrations. Use fixtures to exercise fault paths, missing permissions, inconsistent readings, simulated hardware errors and interrupted runs. Mark simulations visibly.
- Integration-test worker lifecycle, process cleanup, data persistence, cancellation, CLI exit codes, report outputs and release gating. Add CI for build and non-stress tests on Windows/Linux when available. Do not run physical stress tests automatically in CI.
- Validate real detection against known-good hardware and known-fault fixtures or controlled test conditions; do not deliberately damage hardware. Report false positives, coverage limitations and performance overhead. Seek qualified engineering review before unattended burn-in or customer distribution.
- No network traffic, account requirement, telemetry or remote support by default. Support bundles require explicit consent and redaction. No hidden tracking, remote kill switches or automatic uploads.
- Use authentic, functional controls: no decorative buttons that do nothing. If a future module is unavailable, clearly disable it with a reason. No hard-coded positive test results or invented hardware sensor values. No fake progress percentages; indeterminate progress is acceptable when no measured estimate exists.
- Keep a feature/capability matrix distinguishing implemented, tested on fixtures, tested on physical hardware, unsupported and planned. Deliver source, build steps, documentation, test logs and remaining work. Do not promise a fully certified commercial release after a single development pass.

## 14. Initial Claude Code execution prompt

Read this document as the primary project specification. Begin with Milestone 0, then implement Milestone 1 and as much of the remaining roadmap as can be completed and verified responsibly in the available development environment. Use Rust as the required first-party implementation language and FLTK-rs as the initial desktop toolkit. Preserve any existing code for migration; do not silently revert to C++, Qt, Electron or a web stack. Document and narrowly isolate any unavoidable native SDK or GPU-kernel exceptions. If the toolkit cannot meet accessibility or platform requirements, propose an alternative and explain the trade-offs before changing it. Maintain the traditional industrial design exactly as described. Build real diagnostic functionality and do not stop after creating a polished interface. Use fixture-based tests before any physical high-load testing. Run safe build and automated verification commands, document the results and keep the workspace in a usable state. Do not invent passing results or hide blocked features. Ask only when a necessary action requires authorization, a genuinely unavailable dependency, or a consequential decision not covered here. Keep a milestone checklist and record all outstanding hardware validation, licensing and safety requirements.

## 15. Decisions and open items

The user has approved the classic industrial desktop visual direction and the unified OEM validation scope. Rust is the user-requested primary language. FLTK-rs, SQLite, the initial color palette, milestone ordering, error-code scheme and profile durations are proposed implementation defaults rather than verified engineering requirements. FLTK-rs visual fidelity, accessibility, native build dependencies and vendor FFI compatibility need validation. Final exact GPU/CPU support, factory thresholds, test durations, PSU measurement method, commercial licenses, bootable environment, actual BE-01 chassis limits, signing keys and production acceptance policy remain open pending development and hardware validation. No source code, repository changes or hardware tests have been performed by the author of this brief.
