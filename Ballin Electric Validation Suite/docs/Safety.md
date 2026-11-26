# Safety design

1. **Read-only by default.** Inventory, sensors and review tests only read OS interfaces. Low-impact tests are bounded:
   - known-answer CPU runs for seconds;
   - memory buffers are capped at 50% of available memory;
   - scratch files require 1 GiB plus 10% free-space headroom and are always deleted.
2. **No destructive operations.** The release contains no raw disk writes, formatting, firmware flashing, overclocking, voltage or power-limit changes, or security-setting changes. Descriptors with `destructive: true` are refused by preflight.
3. **High-load gating.** Tests with load class `high` (`BE-T-CPU-STRESS`, `BE-T-MEM-STRESS`, `BE-T-SYS-001` and `BE-T-GPU-004`) require all of the following:
   - explicit operator authorization (GUI: *Tests ▸ Authorize High-Load Testing*; CLI: `--authorize-high-load`);
   - a hardware safety profile with `"approved": true` and a named `approved_by`;
   - every `required` sensor rule matching a sensor that currently reports a value;
   - current readings below the warning limits (cool start).

   The worker also refuses high-load tests unless the supervisor sets the authorization flag.
4. **Watchdog during load.** The supervisor samples sensors at `sample_interval_ms` and aborts on:
   - an abort limit (BE-THERM-002);
   - fan speed at or below `abort_below` (BE-THERM-003);
   - a required sensor missing for `max_missed_samples` samples (BE-SNS-001);
   - timeout or heartbeat loss (BE-SYS-002);
   - operator Stop (BE-SYS-003).

   Aborted results are never passed.
5. **No guessed limits.** The repository ships only `profiles/safety/EXAMPLE-not-approved.json`. Its limits are placeholders (0 °C) and it is not approved, so it cannot enable any workload. A qualified engineer must write and approve a device-specific profile from vendor documentation, using conservative limits rather than maximum junction temperatures.
6. **Limits of software.** Software cannot guarantee recovery from kernel or hardware hangs, certify PSU ripple, grounding or mains safety, or replace calibrated thermal and electrical instruments or regulatory testing. Unattended burn-in needs an external watchdog and hardware safeguards.

## Safety profile format

```json
{
  "schema": 1,
  "id": "be01-revA-cpu",
  "version": "1.0.0",
  "description": "...",
  "approved": true,
  "approved_by": "Name, role, date, document reference",
  "sample_interval_ms": 1000,
  "max_missed_samples": 3,
  "cooldown_s": 300,
  "rules": [
    { "matches": "tctl", "kind": "temperature", "warn": 0.0, "abort": 0.0, "required": true },
    { "matches": "cpu_fan", "kind": "fan", "warn": 0.0, "abort_below": 0.0, "required": true }
  ]
}
```

The `0.0` values above are placeholders. Real values must come from the device vendor's documentation and the approved engineering envelope. `matches` is a case-insensitive substring of the sensor id, chip or label (see `bevalidate sensors`).

## Status of physical validation

No high-load workload has been run on any machine during development. Live behavior of the watchdog on real sensors is **unverified**. It is tested only against simulated sensor sequences.
