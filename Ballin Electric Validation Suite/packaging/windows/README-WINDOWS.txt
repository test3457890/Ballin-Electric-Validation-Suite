BE Validation - Ballin Electric System Validation Suite
DEVELOPMENT BUILD (alpha) - NOT FOR PRODUCTION OR FACTORY RELEASE DECISIONS
UNSIGNED: Windows SmartScreen may warn when you start it.

Contents
  bevalidate-gui.exe      Desktop application
  bevalidate.exe          Command-line application
  bevalidate-worker.exe   Isolated diagnostic worker (must stay in this folder)
  profiles\               Test profiles; profiles\safety\ has an EXAMPLE
                          safety profile that is deliberately NOT approved
  docs\                   Operating, hardware testing, troubleshooting and
                          licensing documents

Quick start
  1. Check the ZIP SHA-256 against the published checksum.
  2. Extract the folder anywhere you can write, for example C:\BEValidation.
  3. Double-click bevalidate-gui.exe. Inventory runs automatically (read-only).
  4. Tests > Profile > Quick Diagnostics, then Run Tests (F9).
  Command line: open a terminal in the folder and run
       .\bevalidate.exe inventory
       .\bevalidate.exe run --profile quick --output .\results

No installation, administrator rights, network access or Visual C++ runtime
is required. Data is stored in %LOCALAPPDATA%\BE Validation.

Safety: stress (high-load) tests do not run unless you explicitly authorize
them AND supply an approved, device-specific safety profile. None ships.

This build has been compiled and smoke-tested on GitHub's Windows Server 2022
CI runners only. It has NOT been validated on physical BE-01 hardware or on
any physical workstation. See docs\Known Limitations and Troubleshooting.md.
