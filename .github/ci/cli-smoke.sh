#!/usr/bin/env bash
set -eu
B=target/debug/bevalidate
DB="$RUNNER_TEMP/cli.db"
"$B" --db "$DB" inventory
"$B" --db "$DB" capabilities
set +e
"$B" --db "$DB" run --profile quick --output "$RUNNER_TEMP/quick"
code=$?
set -e
echo "quick profile exit code: $code (0 pass, 1 fail, 2 incomplete; CI runners usually lack sensors, so 2 is expected)"
test "$code" -le 2
test -f "$RUNNER_TEMP/quick/report.html"
