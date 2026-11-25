# `bevalidate` command line

The CLI uses the same engine, safety policy, database and report generator as the GUI. Run `bevalidate <command> --help` for every option.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | All executed tests passed (WARNING and SKIPPED do not fail the exit code; release policy is stricter) |
| 1 | At least one test FAILED |
| 2 | Incomplete: something was NOT RUN, UNSUPPORTED, INCONCLUSIVE or missing |
| 3 | ABORTED (operator stop, timeout, safety abort) |
| 4 | Invalid input (bad profile, unknown unit/run, invalid arguments) |
| 5 | Internal error |
| 6 | Not authorized: release refused or evaluation not eligible |

A successfully written report is not a passing result.

## Common workflows

```bash
bevalidate inventory --format json
```

```bash
bevalidate capabilities
```

```bash
bevalidate tests list
```

```bash
bevalidate profile list
```

```bash
bevalidate profile validate profiles/quick.json
```

```bash
bevalidate run --profile quick --output ./results
```

Factory flow (register the unit, run the acceptance profile, sign off inspections, then review):

```bash
bevalidate unit create --serial BE01-000001 --configuration "BE-01 dual GPU" --technician "A. Tech"
```

```bash
bevalidate run --profile standard --unit BE01-000001 --operator "A. Tech" --output ./results/BE01-000001
```

```bash
bevalidate check --unit BE01-000001 --run-id 12 --check chassis-finish --status pass --operator "Q. Inspector"
```

```bash
bevalidate release evaluate --unit BE01-000001
```

```bash
bevalidate release approve --unit BE01-000001 --reviewer "QA Lead"
```

Rework creates a new linked run; old results are never edited:

```bash
bevalidate service add --unit BE01-000001 --technician "A. Tech" --kind part-replacement --description "Replaced PCIe riser" --part RISER-16X
```

```bash
bevalidate run --profile standard --unit BE01-000001 --rework-of 12
```

High-load runs need explicit authorization and an approved safety profile:

```bash
bevalidate run --profile extended-stability --authorize-high-load --safety-profile ./approved/be01-revA.json
```

Reports for stored runs (PDF is not implemented; it returns exit code 2 with an explanation):

```bash
bevalidate report --run-id 12 --format html --audience customer --out report.html
```

Other commands: `sensors [--watch N]`, `runs`, `events`, `baseline add|list|compare`, `faults`, `audit`, `backup <dest>`, `support-bundle --out file.json`. The support bundle redacts host name, serial numbers and MAC addresses, and is never uploaded.
