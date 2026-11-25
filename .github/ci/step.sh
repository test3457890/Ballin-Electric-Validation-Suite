#!/usr/bin/env bash
# Run a CI step, keep its output, and on failure publish the tail of the
# output as one GitHub annotation (job logs are not always accessible).
name="$1"; shift
mkdir -p ci-logs
log="ci-logs/$name.log"
set +e
"$@" > >(tee "$log") 2>&1
code=$?
set -e
if [ "$code" -ne 0 ]; then
  tail_txt=$(grep -v '^\s*Compiling ' "$log" | tail -n 60)
  tail_txt="${tail_txt//'%'/'%25'}"
  tail_txt="${tail_txt//$'\r'/}"
  tail_txt="${tail_txt//$'\n'/'%0A'}"
  echo "::error title=$name failed (exit $code)::$tail_txt"
fi
exit "$code"
