# Development status

Last updated: 2026-11-25 · Branch `milestone-0-1-foundation` · Engine 0.1.0 · Result schema 1 · DB schema 1

**Bottom line:** The foundation, Linux diagnostics, the supervised worker engine, manufacturing records and both front ends are implemented. They are tested with fixtures, run live on a cloud Linux VM, and run in CI on Ubuntu and Windows Server 2022 (MSVC) GitHub runners, which are virtual machines without real sensors or GPUs. **Nothing has been validated on physical BE-01 hardware or on any physical workstation.** No high-load test has been executed anywhere. GPU compute, VRAM and graphics workloads are not implemented and honestly report UNSUPPORTED.

Definitions used below: *fixture-tested* means simulated inputs in automated tests. *Live (dev VM)* means it executed against the real OS interfaces of the development VM. *Physically verified* means it was confirmed on real target hardware; nothing is in that category yet.

## Milestones

| Milestone | Status | Evidence |
| --- | --- | --- |
| 0 Foundation and approved interface | **Implemented** | Cargo workspace (10 crates), pinned toolchain, FLTK-rs GUI with the approved industrial layout, CLI, SQLite schema, test contracts, capability registry, architecture record. GUI verified under Xvfb by screenshots and xdotool interaction. |
| 1 Real low-impact diagnostics | **Implemented (Linux live on VM; Windows fixture-tested only)** | Native Linux inventory and sensors, safe OS log review, SQLite persistence, JSON/CSV/HTML export, CLI. Known-answer CPU, memory pattern and scratch-integrity tests PASS live on the dev VM. Windows CIM provider fixture-tested and cross-compiled. |
| 2 Stability engine and supervision | **Implemented; fixture-tested** | Isolated workers, scheduler with prerequisites, cancellation, timeouts, heartbeat watchdog, process-group kill, safety preflight, sensor watchdog, failure preservation. Tests cover mismatch, crash, hang/timeout, heartbeat loss, stop, uncooperative stop, missing sensor, simulated over-temperature and oversized IPC. CPU/RAM stress workloads exist but have **not** been run under load (not authorized; no approved safety profile). |
| 3 GPU and system validation | **Partial** | GPU inventory (PCI, NVML runtime-loaded, amdgpu sysfs), PCIe link/AER review, combined CPU+RAM load. **Not implemented:** GPU compute kernels, VRAM tests, graphics workload, simultaneous multi-GPU runs, instrument interfaces. No GPU hardware was available. |
| 4 Manufacturing and service workflow | **Implemented; fixture-tested and live in CLI/GUI** | Units, versioned profiles, manual checklist sign-off, fault catalog, waivers (restricted), hold/rework, linked rework runs, service records, preserved baselines plus comparison, release policy, authorized approval, audit trail, HTML/JSON/CSV reports, customer redaction, backup and interrupted-run recovery. Tests prove failed, incomplete, unsupported, fixture and diagnostic-profile results block release. Native PDF not implemented. |
| 5 Cross-platform packaging and customer diagnostics | **Partial** | Industrial, Classic and Dark themes; zoom; redacted support bundle; unsigned Windows portable ZIP and Linux tar.gz/.deb development builds via the release workflow; CI tests and GUI/CLI smoke runs on Windows Server 2022 (MSVC). **Not done:** installer, signing, physical Windows workstation testing, accessibility testing with assistive technology, customer mode with restricted permissions. |
| 6 Preboot and advanced factory operation | **Not started** | — |

## Commands executed and actual results (2026-11-25, dev VM)

Environment: Ubuntu 24.04.5 LTS, kernel 6.18.44, Intel Xeon (virtualized, 4 vCPU), 16 GiB RAM, virtio disks, no GPU, no hwmon, DMI, EFI or TPM, Rust 1.97.0, CMake 3.28.

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Clean |
| `cargo clippy --workspace --all-targets` | 0 warnings |
| `cargo test --workspace` | **91 passed, 0 failed.** Crate breakdown: be-core 23, be-diagnostics 22, be-platform 7, be-safety 3, be-storage 6, be-reporting 3, be-engine 3; be-worker integration 13 supervision + 6 engine end-to-end; be-cli 5 contract tests. |
| `cargo build --release --target x86_64-pc-windows-gnu -p be-gui -p be-cli -p be-worker` | Builds `bevalidate.exe`, `bevalidate-gui.exe` and `bevalidate-worker.exe` (MinGW cross-check). |
| GitHub CI at `098052f` (PR #2 merge candidate) | **All green.** Ubuntu 24.04 and Windows Server 2022 (MSVC): fmt, clippy `-D warnings`, all tests, GUI smoke start and CLI Quick-profile smoke run. Audit job: cargo-deny and cargo-audit pass. The runners are VMs: no sensors, GPUs or physical hardware. |
| `bevalidate inventory` / `capabilities` | Live. CPU, memory, 6 virtio disks, 11 PCI functions and eth0 detected. DMI, cpufreq, ECC, Secure Boot and all sensors reported **unavailable** with reasons. |
| `bevalidate run --profile quick --unit DEV-VM-0001 --output …` | Live, 19.5 s, exit 2 (INCONCLUSIVE). INV-001 PASS; SNS-001 WARNING (BE-SNS-002, no temperature telemetry); CPU-001 PASS (4 pinned threads, 44 Lucas-Lehmer verifications); MEM-001 PASS (256 MiB, 335,544,320 words verified, 0 mismatches); STO-001 PASS (128 MiB scratch, SHA-256 verified); OS-001 INCONCLUSIVE (BE-OS-002, journal has no kernel log); STO-002, PCIE-001, GPU-001 UNSUPPORTED; NET-001 PASS; FW-001 WARNING (no TPM). Release policy: **not eligible** (correct). |
| `bevalidate-gui --autorun quick` (Xvfb) | Same engine path; run persisted; screenshots reviewed against the approved design reference. |
| `cargo deny check` | advisories ok, bans ok, licenses ok, sources ok (duplicate-version warnings only) |
| `cargo audit` | Initially **1 vulnerability**: RUSTSEC-2026-0459 (bundled SQLite in libsqlite3-sys 0.35, CVE-2026-119375, medium). Fixed by upgrading rusqlite 0.37 → 0.43 (libsqlite3-sys 0.40); re-run reports no vulnerabilities. |

## Known issues and limitations

- `crates/be-gui/src/main.rs` is large and should be split into view modules (maintainability, not correctness).
- Scratch-file read-back may be served from the OS page cache (advisory eviction only); it is not a media scan.
- Memory tests cover only the allocated virtual buffer, not OS-reserved memory.
- Hardware log review uses keyword heuristics and lists events for review only.
- Windows: PCIe link status, ECC/WHEA counters and fast sensor sampling (beyond NVML) are not implemented. CIM queries go through PowerShell, about 1–3 s per inventory.
- Windows Job Object containment is not implemented (current workers spawn no child processes).
- Screen readers are not supported by FLTK (see [Accessibility](Accessibility.md)).
- Reports are not signed or tamper-evident. A hash alone does not prove authenticity.
- No shipping-system integration exists; "Released" is a local record only.

## Requires physical hardware validation (blocking for any support claim)

1. Linux and Windows inventory on BE-01 and representative workstations: DMI serials, NVMe identity, PCIe link widths through risers, dual-GPU identification and slot mapping.
2. Sensor availability and naming on BE-01 (CPU package, VRM, GPU, fans, PSU if exposed), to author the device-specific safety profile.
3. Watchdog behavior with real sensors under controlled, authorized load: abort latency and cooldown behavior.
4. CPU/RAM stress workloads under supervision: thermal behavior, throttling detection, false-positive rate on known-good units.
5. NVML telemetry on RTX PRO 6000 Blackwell Max-Q, and amdgpu telemetry on Radeon AI PRO R9700.
6. SMART/NVMe health via smartctl on actual NVMe devices.
7. Hardware error log review with real MCE, AER or EDAC events (known-fault fixtures or controlled conditions).
8. GUI on physical dual 4K displays, Windows per-monitor DPI, and keyboard-only operation by an operator.
9. All three executables on a physical Windows workstation. So far they have run only on CI virtual machines.

## Open decisions (owner / engineering)

- Approved per-device temperature, fan and power limits (safety profiles) and who approves them.
- BE-01 configuration manifest values (GPU count, link widths, 10GbE) and acceptance thresholds; the factory profile is a DRAFT.
- GPU backend choice (Vulkan compute via `ash` vs CUDA/HIP) and its license review.
- Accessibility path if screen-reader support is required (toolkit change needs approval).
- Project license, code-signing keys and distribution channel.
- Windows release toolchain (MSVC recommended).
- Whether to enable the CI workflow on push (currently manual `workflow_dispatch` only).

## Next steps

1. Milestone 3: Vulkan compute backend (`ash`) with deterministic known-answer kernels and VRAM pattern tests per device, after license review and with access to GPU hardware.
2. Run on a physical Linux workstation with hwmon sensors. Draft a safety profile for engineering approval.
3. Execute and fix the Windows build on real Windows. Add WHEA log parsing and PCIe link data via SetupAPI.
4. Customer mode (restricted actions), installer packaging, signed reports.
5. Milestone 6: bootable Linux USB prototype reusing profiles and result schema.

## License and advisory checks

Dependency licenses were reviewed from `cargo metadata`. All 78 third-party crates are permissive (see [Third Party Notices](Third%20Party%20Notices.md)). cargo-deny and cargo-audit pass as recorded above. Re-run both before every release.
