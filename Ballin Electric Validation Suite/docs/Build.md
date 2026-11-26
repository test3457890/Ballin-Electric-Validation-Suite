# Building, testing and running

All commands are run from the `Ballin Electric Validation Suite` folder, which is the Cargo workspace root. The toolchain is pinned in `rust-toolchain.toml` (Rust 1.97.0 stable). `Cargo.lock` is checked in.

## Native build dependencies

FLTK (C++) is compiled from source by `fltk-sys` using CMake. SQLite is compiled from the bundled amalgamation by `libsqlite3-sys`.

**Linux (Ubuntu 24.04, verified):**

```bash
sudo apt-get install -y build-essential cmake pkg-config libx11-dev libxext-dev libxft-dev libxinerama-dev libxcursor-dev libxrender-dev libxfixes-dev libfontconfig-dev libpango1.0-dev libcairo2-dev libglu1-mesa-dev
```

Optional at runtime: `smartmontools` (BE-T-STO-002), the NVIDIA driver (NVML telemetry), and `journalctl` access through the `adm` or `systemd-journal` group (BE-T-OS-001).

**Windows:** install Visual Studio 2022 Build Tools ("Desktop development with C++") and CMake, then run the standard `cargo build`. CI builds and tests on GitHub's `windows-2022` runners with MSVC. For downloadable builds the release workflow sets `RUSTFLAGS=-C target-feature=+crt-static` (no `--target` flag), so the Rust, FLTK and SQLite code links the C runtime statically, and checks with `dumpbin /dependents` that no MSVC runtime DLL is required.

## Commands

```bash
cargo fmt --all -- --check
```

```bash
cargo clippy --workspace --all-targets
```

```bash
cargo test --workspace
```

```bash
cargo build --release --workspace
```

The executables land in `target/release/`: `bevalidate` (CLI), `bevalidate-gui` (desktop) and `bevalidate-worker` (isolated test worker). The worker must sit next to the CLI/GUI, or `BE_WORKER_PATH` must point to it.

Cross-compile check for Windows from Linux (requires `gcc-mingw-w64-x86-64` and `g++-mingw-w64-x86-64`):

```bash
rustup target add x86_64-pc-windows-gnu
```

```bash
cargo build --release --target x86_64-pc-windows-gnu -p be-gui -p be-cli -p be-worker
```

## Running

```bash
./target/release/bevalidate-gui
```

```bash
./target/release/bevalidate run --profile quick --output ./results
```

The database defaults to `~/.local/share/be-validation/be-validation.db` on Linux and `%LOCALAPPDATA%\BE Validation\be-validation.db` on Windows. Override it with `--db PATH` or `BE_VALIDATION_DB`.

GUI options for automation and screenshots: `--db PATH`, `--theme industrial|classic|dark`, `--autorun <profile-id>` (same path as F9), `--exit-after <seconds>`. `--fixture-mode` is for development only: simulated tests appear and every result is labeled as a fixture.

## Safe automated verification

`cargo test --workspace` runs:
- unit tests;
- fixture tests (synthetic sysfs trees, CIM JSON, smartctl JSON, injected memory faults, simulated sensors);
- supervisor integration tests using fixture workers;
- a small number of low-impact live checks: CPU known-answer with 2 threads and small exponents, and a 16 MiB memory pattern.

No stress test runs in automated testing. High-load tests only run with `--authorize-high-load` and an approved safety profile.

## Continuous integration

`.github/workflows/ci.yml` runs on every push to `main`, `milestone-*` and `feature-*` branches and on pull requests. It covers Ubuntu 24.04 and Windows Server 2022 (MSVC):

- format check;
- clippy with `-D warnings`;
- `cargo test --workspace --locked`;
- a build of all executables;
- a GUI smoke start (Xvfb on Linux);
- a CLI smoke run of the Quick profile.

A separate job runs `cargo deny check` and `cargo audit`. When a step fails, `.github/ci/step.sh` publishes the tail of its output as a check-run annotation.

## Development builds (downloads)

`.github/workflows/release.yml` runs when a `v*` tag is pushed (for example `v0.1.0-alpha`), or when a branch named `release/<label>` is pushed (for example `release/v0.1.0-alpha`; the workflow then creates the tag itself). Only prerelease labels (`-alpha`, `-beta`, `-rc`, `-dev`) can be published, and the label needs a matching `docs/release-notes/<label>.md`. It:

1. runs the tests on Windows;
2. builds the release executables with MSVC and a static CRT;
3. checks DLL dependencies and smoke-tests the GUI and CLI;
4. packages the Windows portable ZIP (`packaging/windows/build-release.ps1`) and the Linux tar.gz and .deb (`packaging/linux/build-release.sh`);
5. publishes everything as a GitHub **prerelease** with `SHA256SUMS.txt`.

The builds are **unsigned development builds**; see [Windows Development Build](Windows%20Development%20Build.md).

Local packaging:

```bash
./packaging/linux/build-release.sh v0.1.0-dev
```

```powershell
.\packaging\windows\build-release.ps1 -Label v0.1.0-dev
```
