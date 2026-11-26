#!/usr/bin/env bash
# Build an unsigned Linux release archive. Does not install anything system-wide.
# Code signing and distribution are NOT implemented (no keys exist).
set -euo pipefail
cd "$(dirname "$0")/../.."
cargo build --release --locked -p be-cli -p be-gui -p be-worker
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
OUT="dist/be-validation-${VERSION}-linux-$(uname -m)"
rm -rf "$OUT" && mkdir -p "$OUT/profiles/safety" "$OUT/docs"
cp target/release/bevalidate target/release/bevalidate-gui target/release/bevalidate-worker "$OUT/"
cp profiles/*.json "$OUT/profiles/"
cp profiles/safety/EXAMPLE-not-approved.json "$OUT/profiles/safety/"
cp README.md "$OUT/"
cp "docs/Third Party Notices.md" docs/Safety.md docs/CLI.md "docs/Fault Codes.md" docs/Accessibility.md "$OUT/docs/"
tar -C dist -czf "$OUT.tar.gz" "$(basename "$OUT")"
sha256sum "$OUT.tar.gz" > "$OUT.tar.gz.sha256"
echo "Built $OUT.tar.gz (UNSIGNED; a hash alone does not prove authenticity)"
