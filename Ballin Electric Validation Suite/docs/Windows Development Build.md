# Windows development build: installation and operation

The development build is an **alpha** for evaluation and hardware bring-up. It is unsigned and not production-ready. It has been compiled and smoke-tested only on GitHub-hosted Windows Server 2022 runners. **It has not been validated on physical BE-01 hardware or on any physical workstation.** Do not base shipment decisions on it.

## Download

Downloads are on the repository's **Releases** page, under the prerelease labeled `v0.1.0-alpha`:

| File | Purpose |
| --- | --- |
| `BE-Validation-v0.1.0-alpha-windows-x64-portable.zip` | Portable Windows x64 edition: GUI, CLI and worker |
| `…-windows-x64-portable.zip.sha256` | SHA-256 of the ZIP |
| `BE-Validation-v0.1.0-alpha-linux-x86_64.tar.gz`, `be-validation_*.deb` | Linux development packages |
| `SHA256SUMS.txt` | Checksums of all release files |

GitHub Actions also keeps the same files as workflow artifacts for 90 days.

## Verify the download

In PowerShell, in the download folder:

```powershell
Get-FileHash .\BE-Validation-v0.1.0-alpha-windows-x64-portable.zip -Algorithm SHA256
```

Compare the hash with the `.sha256` file or `SHA256SUMS.txt`. A matching hash only shows the file is intact. Without a code signature it does not prove who built it.

## Requirements

- Windows 10 or 11, or Windows Server 2019 or later, x64.
- No installation, Visual C++ Redistributable or network connection is needed. The C runtime is statically linked, and the release pipeline checks that the executables do not depend on the MSVC runtime DLLs.
- Optional:
  - an NVIDIA driver, for NVML GPU telemetry;
  - smartmontools installed in `C:\Program Files\smartmontools`, for SMART health;
  - administrator rights, for Secure Boot, TPM and ACPI thermal readings.

## Install (portable)

1. Right-click the ZIP, open **Properties**, and tick **Unblock** if it is offered. This reduces SmartScreen prompts.
2. Extract it to a folder you can write to, such as `C:\BEValidation`. Keep `bevalidate-worker.exe` next to the other two executables.
3. Start `bevalidate-gui.exe`. Because the build is unsigned, SmartScreen may show *Windows protected your PC*. Choose **More info**, then **Run anyway**, only if the checksum matched.

Data (SQLite records and printable reports) is stored in `%LOCALAPPDATA%\BE Validation`. To uninstall, delete the folder, and also that data folder if the records are no longer needed. Back them up first with *File ▸ Database Backup*.

## Operate

**Desktop application:**
1. The hardware inventory scan runs read-only at start. Review **Overview**, **Hardware** and **System Navigator ▸ Unavailable Data**. On Windows several items are reported unavailable; see the limitations document.
2. Choose *Tests ▸ Profile ▸ Quick Diagnostics*, then **Run Tests** (F9). **Stop** (Shift+F5) is always available.
3. Inspect results on **Tests** and **Errors**. Export with *File ▸ Export Report* (HTML), *Save* (JSON) or *Export CSV*.
4. Manufacturing workflow: open the **Manufacturing** tab, choose *New Unit*, then *Use for Runs*. Run the **Standard Validation** profile, then use *Inspection…*, *Evaluate* and, only when eligible, *Approve Release…*.

**Command line** (PowerShell in the extracted folder):

```powershell
.\bevalidate.exe inventory
```

```powershell
.\bevalidate.exe capabilities
```

```powershell
.\bevalidate.exe run --profile quick --output .\results
```

Exit codes and all commands are listed in `docs\CLI.md`.

## Stress testing

Stress tests (CPU, RAM and combined load) are blocked until both of the following are in place:
- explicit operator authorization;
- a device-specific safety profile approved by a named engineer.

The included `profiles\safety\EXAMPLE-not-approved.json` cannot enable them. See `docs\Safety.md` and `docs\Physical Hardware Testing Guide.md`.

## Linux development package

```bash
sha256sum -c BE-Validation-v0.1.0-alpha-linux-x86_64.tar.gz.sha256
```

```bash
tar xzf BE-Validation-v0.1.0-alpha-linux-x86_64.tar.gz
```

```bash
./BE-Validation-v0.1.0-alpha-linux-x86_64/bevalidate-gui
```

Or install the `.deb` with `sudo apt install ./be-validation_*.deb`. It installs to `/opt/be-validation`, provides the `bevalidate` and `bevalidate-gui` commands, and adds a desktop entry.
