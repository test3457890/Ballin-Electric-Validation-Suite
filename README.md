<p align="center">
  <img src="repository-assets/Brand%20Banner.svg" alt="Ballin Electric System Validation Suite" width="100%">
</p>

# Ballin Electric System Validation Suite

Professional hardware diagnostics, stability testing, factory burn-in and manufacturing quality assurance.

[Interface preview](#interface-preview) · [Capabilities](#planned-capabilities) · [Factory workflow](#factory-workflow) · [Development documentation](#development-documentation)

> Development project. This repository contains the product specification, the approved interface concept and an early Rust implementation (engine 0.1.0) that has been tested with fixtures and on a cloud Linux development VM only. The capabilities below are development targets, not claims of a released or physically validated application. Consult the development status document and capability matrix for implementation evidence.

## Overview

BE Validation is a Rust-first engineering application intended for OEM manufacturers, workstation integrators, repair technicians and research laboratories. Its purpose is to detect hardware faults, verify system stability and provide traceable evidence for factory acceptance and later service.

Performance benchmarking is only one part of the system. A passing benchmark does not establish hardware reliability or authorize shipment.

## Interface preview

![Approved industrial desktop interface concept showing a GPU memory failure, event log and shipment hold](Ballin%20Electric%20Validation%20Suite/design/assets/Main%20Window%20Reference.png)

*Approved static design reference. All readings and test results in the image are simulated, not actual application output.*

Traditional desktop menus, hardware navigation trees, sortable data tables, tabbed inspection panels and a persistent diagnostic event log. Compact controls and restrained colors take precedence over decorative dashboard layouts.

[Full-size PNG](Ballin%20Electric%20Validation%20Suite/design/assets/Main%20Window%20Reference.png) · [Editable SVG](Ballin%20Electric%20Validation%20Suite/design/assets/Main%20Window%20Reference.svg) · [Visual design language](Ballin%20Electric%20Validation%20Suite/design/Design%20Language.md)

## Planned capabilities

| Module | Intended scope |
| :-- | :-- |
| System inventory | CPU, memory, GPUs, storage, motherboard, firmware and connected devices |
| Hardware diagnostics | Known-answer tests, error detection, evidence collection and targeted follow-up |
| Stability & burn-in | CPU, RAM, GPU, VRAM and combined system workloads with controlled safety limits |
| Live monitoring | Temperature, clock, fan and power telemetry where supported |
| Factory quality control | Build records, test profiles, manual inspection, rework and shipment holds |
| Service & troubleshooting | Diagnostic codes, fault isolation, baseline comparison and service history |
| Reporting | Traceable validation records and printable reports |
| Preboot diagnostics | OS-independent checks and recovery environment, subject to implementation |

Windows and Linux are target platforms. Support for individual devices, drivers, APIs and test backends must be demonstrated before it is claimed.

## Factory workflow

![Proposed manufacturing acceptance process with explicit hold and release review paths](repository-assets/Factory%20Workflow.svg)

Required failed, missing or unsupported tests must prevent factory release. A successful test sequence is evidence for an authorized human release decision, not electrical safety certification.

## Technical direction

| Area | Selected direction |
| :-- | :-- |
| Language | Rust, Cargo workspace |
| Desktop GUI | FLTK-rs, traditional native-style controls |
| Architecture | Modular platform providers, isolated test workers and independent safety supervision |
| Local storage | SQLite via rusqlite |
| Connectivity | Offline-first operation |
| Platforms | Windows and Linux targets |

The complete architecture, milestones, safety controls and acceptance criteria are defined in the development specification.

## Development documentation

The original development package remains in the [Ballin Electric Validation Suite directory](Ballin%20Electric%20Validation%20Suite/).

| Document | Purpose |
| :-- | :-- |
| [Claude Code instructions](Ballin%20Electric%20Validation%20Suite/CLAUDE.md) | Coding agent rules and development entry point |
| [Development specification](Ballin%20Electric%20Validation%20Suite/docs/Development%20Specification.md) | Technical requirements and implementation milestones |
| [Development status](Ballin%20Electric%20Validation%20Suite/docs/Development%20Status.md) | Implementation evidence and outstanding work |
| [Design language](Ballin%20Electric%20Validation%20Suite/design/Design%20Language.md) | Approved interface requirements |
| [Design tokens](Ballin%20Electric%20Validation%20Suite/design/Design%20Tokens.json) | Machine-readable typography, color and spacing rules |
| [Third-party notices](Ballin%20Electric%20Validation%20Suite/docs/Third%20Party%20Notices.md) | Dependency and licensing review |
| [Visual asset gallery](repository-assets/README.md) | Branding, UI reference and workflow artwork |

## Development and safety

Read the development specification and Claude instructions before implementing or running tests. Use safe fixtures and low-impact checks for automated development tests. Do not run intensive physical stress tests without authorization, flash firmware, modify voltages or perform destructive disk tests without the required approvals.

The software is not a replacement for physical inspection, calibrated test equipment or regulatory certification. Unsupported readings and failed tests must remain explicit; never fabricate results.

## License and distribution

No project license has been selected. This is a private development project; do not redistribute the repository, its assets or proprietary materials without authorization. Third-party dependencies remain subject to their respective licenses.
