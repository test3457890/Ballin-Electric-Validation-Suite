# Physical hardware testing guide (development builds)

This guide is for engineers validating BE Validation on real hardware for the first time. Nothing in the software has been validated on physical hardware yet. The goal is to collect evidence, find false positives and missing telemetry, and produce the device-specific safety profile needed before any stress testing.

## Ground rules

- Use known-good hardware first, then known-fault fixtures under controlled conditions. Never damage hardware deliberately.
- Read-only and low-impact tests may run unattended. **Stress tests require a qualified engineer present, an approved safety profile and explicit authorization.**
- Keep the hardware's own thermal protections enabled. BE Validation never changes firmware, voltage, power-limit or fan settings.
- Record ambient temperature, power source, BIOS settings (stock) and driver versions for every session.
- Do not use customer data or customer machines without consent. Collect support bundles with *System ▸ Create Support Bundle*; they redact host name, serial numbers and MAC addresses.

## Phase A: read-only bring-up (no load)

For each machine (Windows and Linux):

1. Run `bevalidate inventory --format json > inventory.json` and `bevalidate capabilities --format json > caps.json`. Compare against the actual hardware:
   - CPU model and core count;
   - memory size;
   - every NVMe device (model, serial, firmware);
   - each GPU by PCI address and slot;
   - NICs and link speeds;
   - BIOS version, Secure Boot and TPM.

   Record every mismatch or "unavailable" item that should be available.
2. Run `bevalidate sensors --watch 60`. Note each sensor's id and label, units and plausibility against a trusted tool or instrument. This list is the input for the safety profile.
3. Run the **Quick Diagnostics** profile, and on Windows also as administrator. Expected: no FAIL on known-good hardware. Investigate every WARNING and INCONCLUSIVE.
4. Run **Standard Validation**. GPU compute and VRAM steps report UNSUPPORTED until the GPU backend lands; that is expected and correctly blocks release.
5. Repeat Quick Diagnostics three times to check repeatability of results and durations.

Deliverable: a table of inventory accuracy, sensor coverage and any false positives per machine.

## Phase B: safety profile authoring (engineering)

1. Map each critical component to a sensor id from Phase A: CPU package or Tctl, GPU, VRM if exposed, NVMe, and the CPU and chassis fans.
2. Take warning and abort limits from **vendor documentation and the approved thermal design**, choosing conservative values well below maximum junction temperatures. Do not copy numbers from other machines.
3. Write the profile following `docs/Safety.md`. Mark sensors that must exist and stay fresh as `"required": true`.
4. Have a qualified engineer review it, then set `"approved": true` and `"approved_by"` to a name, role, date and document reference. Store it under change control. It is not shipped in the repository.

## Phase C: supervised load bring-up (authorized only)

Only with the approved profile and an engineer present:

1. Begin with a short `BE-T-CPU-STRESS` run: a custom profile with `"duration_s": 120`. Watch the event log and an independent thermal reading.
2. Check that the warning and abort behavior fires. Use a deliberately *lowered* abort limit in a test copy of the profile (still approved for test use), for example a few °C above idle. Confirm the workload stops (BE-THERM-002), the result is ABORTED, and the unit cools down.
3. Increase duration step by step: 10 min, 1 h, then the planned profile durations. Record the steady-state temperatures, clocks and throttling.
4. Repeat for `BE-T-MEM-STRESS` and `BE-T-SYS-001`.
5. On a known-good unit, any FAIL is a false positive to investigate before going further.

## Phase D: fault detection evidence

Use known-fault fixtures only (for example a known-bad DIMM, a deliberately mis-seated riser on a test bench, or a degraded NVMe), with power removed for all hardware changes. Record whether BE Validation detects each fault, which code it reports, and whether its hypotheses and next steps were useful.

## Reporting results

For each session, keep:
- the HTML report and `report.json`;
- the support bundle;
- photos of the setup;
- independent instrument readings;
- notes.

File issues with the exact build label (`BUILD-INFO.txt`), OS version and steps to reproduce. Update `docs/Capability Matrix.md` by changing a feature's *Physical* column only with this evidence.
