# Known limitations and troubleshooting (v0.1.0-alpha)

## Known limitations

**Validation status**
- No validation on physical BE-01 hardware or any physical workstation. Windows builds are tested only on GitHub CI runners (Windows Server 2022, virtual machines without real sensors or GPUs).

**GPU**
- GPU compute, VRAM and graphics stress (FurMark-style) tests are not implemented in this build. They report UNSUPPORTED, which blocks release under the Standard and Factory profiles.

**Stress testing**
- Stress tests cannot run without an approved, device-specific safety profile. None ships.

**Windows-specific**
- Inventory uses CIM queries through Windows PowerShell, which take about 1–3 s.
- PCIe link status and ECC counters are not implemented.
- Temperatures come only from ACPI thermal zones (usually needs administrator) and NVML.
- Fan, voltage and VRM sensors are not available.
- Worker processes are terminated directly; Job Object containment is planned.

**Linux-specific**
- Sensors depend on kernel drivers (hwmon, k10temp, coretemp, nct67xx and similar).
- Reading the kernel journal may require the `adm` or `systemd-journal` group.

**Coverage limits**
- Memory tests cover only the allocated buffer, not memory reserved by the OS.
- Scratch-file read-back may be served from the OS cache.
- Storage health needs smartmontools, usually run as administrator or root.

**Not implemented**
- Native PDF reports: use the HTML report and print it.
- Signed reports.
- An installer: the build is portable only.
- Display, USB and network throughput tests.
- Preboot diagnostics.

**User interface**
- Screen readers are not supported by the FLTK toolkit.
- Operator and reviewer identities are typed in, not authenticated. Protect stations with OS accounts.

## Troubleshooting

| Symptom | Cause / fix |
| --- | --- |
| SmartScreen: *Windows protected your PC* | The build is unsigned. Verify the SHA-256, then choose *More info ▸ Run anyway*. |
| Tests immediately show NOT RUN with BE-SYS-001 "Worker could not be started" | `bevalidate-worker(.exe)` is missing from the executable's folder, or was blocked by antivirus. Keep all three executables together, or set `BE_WORKER_PATH`. |
| Every step shows ABORTED with BE-SYS-002 | The worker stopped responding (heavy system load, debugger, antivirus scanning). Re-run; if it repeats, collect a support bundle. |
| "Database unavailable" in the status bar | The data directory is not writable, or the database was created by a newer version (refused for safety). Check `%LOCALAPPDATA%\BE Validation` or `~/.local/share/be-validation`; restore from backup if needed. |
| Sensors tab: "No sensors available" | The platform exposes none through supported interfaces. Linux: load the hwmon drivers (`sensors-detect` from lm-sensors). Windows: run as administrator for ACPI zones; NVIDIA GPUs need the NVIDIA driver. |
| Storage Health is UNSUPPORTED | Install smartmontools (Windows: default path `C:\Program Files\smartmontools`) and run elevated. Virtual disks have no SMART data. |
| Hardware Error Log Review is INCONCLUSIVE (BE-OS-002) | The log is not readable. Linux: add the user to `adm` or `systemd-journal`. Windows: check that the System event log service is running. |
| High-load test NOT RUN with BE-SAF-001 | This is by design. Authorization and an approved safety profile are required (see `docs/Safety.md`). |
| A run was marked interrupted (BE-PWR-001) at start-up | The previous run ended without completing (crash, power loss or forced exit). Results stay incomplete and must be re-run. |
| Release approval refused | The policy found blockers (failed, missing or unsupported required tests; unsigned inspections; fixture or diagnostic runs). Use *Evaluate* to list them. |
| Window too small or text too small on 4K | Use *View ▸ Zoom In* (Ctrl+=). The minimum window size is 1024 × 768. |

Report problems with the build label from `BUILD-INFO.txt`, the OS version, a support bundle and the steps to reproduce.
