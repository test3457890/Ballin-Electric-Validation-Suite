# Changelog

All notable changes to BE Validation. Versions before 1.0 are development builds and are not production releases.

## [Unreleased]

### Added
- Vulkan GPU diagnostic engine (`be-gpu`, ash):
  - compute known-answer test checked against a CPU reference;
  - VRAM moving-inversion pattern test with fault capture;
  - original FurMark-inspired graphics stress with bit-exact reference-frame checks;
  - independent and simultaneous multi-GPU modes;
  - NVML and amdgpu telemetry with throttle-reason and PCIe-replay detection;
  - new fault codes BE-GPU-002, 010, 011, 012, 021 and 030, and BE-PCIE-004.

  Tested on a software Vulkan device only.
- Windows x64 portable development build (MSVC, static CRT) and Linux tar.gz/.deb packages, built by `.github/workflows/release.yml` and published as a GitHub *prerelease* with SHA-256 checksums.
- Documentation: Windows installation and operation, physical hardware testing guide, known limitations and troubleshooting.

### Changed
- Release binaries keep line tables only (smaller downloads, readable backtraces).

## [0.1.0-alpha] - foundation (merged in PR #2)

### Added
- Rust workspace:
  - core models, fault catalog, profiles and release policy;
  - Linux and Windows platform providers;
  - NVML telemetry;
  - diagnostics: CPU known-answer, RAM patterns, scratch integrity, and log, PCIe, network, firmware, sensor and GPU reviews;
  - isolated worker supervisor with safety watchdog;
  - SQLite records;
  - HTML/JSON/CSV reporting;
  - `bevalidate` CLI;
  - FLTK-rs industrial GUI.
- CI on Ubuntu and Windows (MSVC): fmt, clippy `-D warnings`, tests, GUI and CLI smoke tests, cargo-deny and cargo-audit.

### Security
- Bounded reads of operator-supplied files, atomic writes, private report directory, external tools resolved from system paths only.
- Upgraded rusqlite to 0.43 (bundled SQLite fix for RUSTSEC-2026-0459).
