# Capability matrix

Legend:
- **Impl**: implemented in code.
- **Fixture**: covered by automated fixture or unit tests (simulated inputs; not hardware evidence).
- **Dev VM**: executed live on the development machine (cloud Linux VM, Intel Xeon 4 vCPU, 16 GiB, virtio disks, no GPU, no hwmon/DMI/EFI/TPM). It shows the code runs on real OS interfaces, not that hardware is supported.
- **Physical**: verified on physical target hardware.

No feature has been verified on physical BE-01 hardware, on any physical workstation, or on Windows. Every "Physical" cell is **No**.

| Capability | Test ID / location | Impl | Fixture | Dev VM | Physical | Notes / limitations |
| --- | --- | --- | --- | --- | --- | --- |
| Linux inventory (CPU, memory, storage, PCI, network, USB, DMI, firmware) | `be-platform::linux` | Yes | Yes | Yes (no DMI/EFI/TPM present on VM: reported unavailable) | No | Serial numbers need root |
| Linux sensors (hwmon temp/fan/voltage/power/current, thermal zones, cpufreq) | `be-platform::linux::sensors` | Yes | Yes | Ran; VM exposes none (reported unavailable) | No | Depends on kernel drivers |
| Windows inventory (CIM: CPU, OS, memory, disks, GPUs, NICs, ACPI thermal, Secure Boot, TPM) | `be-platform::windows_cim` | Yes | Yes (JSON parsing) | Runs in CI on Windows Server 2022 runners (virtual machines without real sensors or GPUs) | No | Not run on a physical Windows workstation. PCIe link and ECC are not implemented on Windows |
| NVIDIA telemetry (NVML, runtime-loaded) | `be-platform::gpu` | Yes | Bus-ID normalization only | NVML absent: reported unsupported | No | Untested with an NVIDIA driver |
| AMD GPU VRAM accounting (amdgpu sysfs) | `be-platform::linux` | Yes | Yes | n/a | No | |
| Hardware inventory & manifest check | BE-T-INV-001 | Yes | Yes | Yes | No | Manifest values for BE-01 are DRAFT |
| Sensor survey & freshness | BE-T-SNS-001 | Yes | Yes | Yes (WARNING: no temperature telemetry) | No | |
| CPU known-answer (NIST SHA-256, Lucas-Lehmer, exact FMA matmul, per-CPU pinned threads) | BE-T-CPU-001 | Yes | Yes | Yes (PASS, 44 LL verifications) | No | Pinning on Linux only |
| CPU verified stress (Prime95-style) | BE-T-CPU-STRESS | Yes | Refusal path tested | Not run (high load; not authorized) | No | Needs authorization and an approved safety profile |
| RAM pattern test (8 patterns, volatile, allocated buffer) | BE-T-MEM-001 | Yes | Yes (injected bit flips detected) | Yes (PASS, 256 MiB) | No | No OS-reserved memory coverage |
| RAM extended integrity | BE-T-MEM-STRESS | Yes | Refusal path | Not run (high load) | No | |
| Combined CPU + RAM load | BE-T-SYS-001 | Yes | Refusal path | Not run (high load) | No | Does not load GPUs |
| Storage scratch-file integrity | BE-T-STO-001 | Yes | Free-space path | Yes (PASS, 128 MiB) | No | Read-back may come from OS cache |
| Storage health (smartctl JSON) | BE-T-STO-002 | Yes | Yes (JSON evaluation) | UNSUPPORTED (virtual disks) | No | Requires smartmontools and usually root |
| Hardware error log review (journal / WHEA) + EDAC ECC | BE-T-OS-001 | Yes | Yes (journal keywords; WHEA event IDs 17/18/19/47) | INCONCLUSIVE (journal has no kernel log on VM) | No | Journal keyword classification is heuristic. WHEA query untested on a physical Windows machine |
| PCIe link & AER review | BE-T-PCIE-001 | Yes (Linux) | Yes | UNSUPPORTED (no link data on VM) | No | Idle downshift only warns; manifest violations fail |
| Network link review | BE-T-NET-001 | Yes | Yes | Yes (PASS) | No | No throughput test |
| Firmware & security review | BE-T-FW-001 | Yes | Yes | Yes (WARNING: no TPM) | No | Read-only |
| GPU inventory & telemetry | BE-T-GPU-001 | Yes | Yes | UNSUPPORTED (no GPU) | No | |
| Vulkan GPU backend (ash, runtime-loaded) | `be-gpu` | Yes | Yes, on Mesa lavapipe (a **software** Vulkan device) | Lavapipe only; VM has no GPU | No | Never run on NVIDIA, AMD or Intel GPU hardware |
| GPU compute verification (CPU-reference known answer) | BE-T-GPU-002 | Yes | Yes (lavapipe; mismatch path via unit tests) | Lavapipe only | No | Software devices excluded unless `include_software` is set; otherwise UNSUPPORTED (BE-GPU-002) |
| GPU VRAM pattern test (6 patterns, moving inversions) | BE-T-GPU-003 | Yes | Yes (lavapipe; an injected bit flip is detected at the exact offset) | Lavapipe only | No | Covers only allocatable memory (≤50% of heap); ECC counters not read |
| GPU graphics stress (original FurMark-inspired shader) | BE-T-GPU-004 | Yes | Yes (lavapipe, short authorized run in tests) | Not run under load | No | High load: needs authorization and an approved safety profile; offscreen only |
| Simultaneous multi-GPU runs | `mode: simultaneous` | Yes | Single software device only | — | No | Needs a multi-GPU machine |
| GPU telemetry: temperature, power, clocks, throttle reasons, PCIe replay | NVML / amdgpu sysfs | Yes | Aggregation logic unit-tested | No GPU on VM | No | Intel GPU telemetry is not implemented |
| CUDA / HIP backends | — | **Planned** | — | — | — | The backend-neutral `be-gpu` API is ready for them |
| Worker isolation, heartbeat, timeout, crash, stop, kill | `be-safety::supervisor` | Yes | Yes (13 integration tests) | Yes | No | Windows: kill-on-close Job Object contains the worker tree (CI runners only) |
| Safety preflight and sensor watchdog (over-temperature / sensor loss abort) | `be-safety` | Yes | Yes (simulated sensors) | No live high-load run | No | No approved safety profile exists yet |
| Profiles (quick, standard, extended-stability, factory-burn-in DRAFT) | `profiles/` | Yes | Validation tested | quick run live | No | Durations and limits are proposals |
| Release policy and gating | `be-core::policy`, `be-engine` | Yes | Yes | Yes (quick run correctly not eligible) | No | No shipping-system integration |
| SQLite records, immutability, migrations, recovery, backup | `be-storage` | Yes | Yes | Yes | No | |
| Manufacturing records (units, inspections, waivers, holds, rework, service, baselines) | CLI + GUI | Yes | Yes | Yes (CLI) | No | |
| Reports: HTML (internal/customer), JSON, CSV | `be-reporting` | Yes | Yes | Yes | No | Not signed / tamper-evident yet |
| Native PDF reports | `be-reporting::pdf` | Yes | Yes (structure; validated with poppler `pdfinfo`) | Yes | No | Not signed |
| Fault isolation (observations → likely causes → follow-up tests) | `be-core::isolation`, `bevalidate analyze` / `follow-up` | Yes | Yes (rule unit tests) | Yes | No | Rule-based hints with stated confidence, not a diagnosis. Follow-up profiles are never release-eligible and exclude high load |
| Restricted customer mode | `--customer`, `BE_VALIDATION_MODE=customer`, `customer-edition` feature | Yes | Yes (CLI refusal tests) | Yes (GUI under Xvfb) | No | A convenience restriction, **not** a security boundary |
| Signed / tamper-evident records | — | **Planned** | — | — | — | Needs a key-management decision; no keys generated |
| CLI `bevalidate` with exit codes | `be-cli` | Yes | Yes | Yes | No | |
| GUI (Industrial, Classic, Dark; zoom; keyboard row navigation) | `be-gui` | Yes | — | Yes, under Xvfb (screenshots) | No | Screen-reader support unavailable (see Accessibility) |
| Support bundle (redacted, local only) | CLI + GUI | Yes | — | Yes | No | |
| Display EDID / test patterns, USB/audio fixtures, network throughput | — | **Planned** | — | — | — | |
| Preboot environment | — | **Planned** | — | — | — | Milestone 6 |
