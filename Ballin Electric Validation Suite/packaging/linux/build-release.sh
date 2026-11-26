#!/usr/bin/env bash
# Build UNSIGNED Linux development packages: a portable tar.gz and a .deb.
# Usage: packaging/linux/build-release.sh [label, e.g. v0.1.0-alpha]
# Code signing is NOT performed (no signing credentials exist).
set -euo pipefail
cd "$(dirname "$0")/../.."
cargo build --release --locked -p be-cli -p be-gui -p be-worker
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
LABEL="${1:-v$VERSION-dev}"
case "$LABEL" in v*) ;; *) LABEL="v$VERSION-dev" ;; esac
ARCH=$(uname -m)
NAME="BE-Validation-$LABEL-linux-$ARCH"
OUT="dist/$NAME"
rm -rf "$OUT" && mkdir -p "$OUT/profiles/safety" "$OUT/docs"
cp target/release/bevalidate target/release/bevalidate-gui target/release/bevalidate-worker "$OUT/"
cp profiles/*.json "$OUT/profiles/"
cp profiles/safety/EXAMPLE-not-approved.json "$OUT/profiles/safety/"
cp README.md "$OUT/"
cp "docs/Third Party Notices.md" docs/Safety.md docs/CLI.md "docs/Fault Codes.md" docs/Accessibility.md \
   "docs/Physical Hardware Testing Guide.md" "docs/Known Limitations and Troubleshooting.md" docs/Build.md "$OUT/docs/"
printf 'BE Validation %s\nDEVELOPMENT BUILD - NOT FOR PRODUCTION USE - UNSIGNED\nCommit: %s\n' "$LABEL" "${GITHUB_SHA:-local}" > "$OUT/BUILD-INFO.txt"
tar -C dist -czf "dist/$NAME.tar.gz" "$NAME"
(cd dist && sha256sum "$NAME.tar.gz" > "$NAME.tar.gz.sha256")

# Debian package: installs to /opt/be-validation with launchers in /usr/bin.
DEBVER="${VERSION}~${LABEL#v$VERSION}"; DEBVER="${DEBVER%~}"; DEBVER="${DEBVER//-/}"
PKG="dist/deb/be-validation_${DEBVER}_amd64"
rm -rf "$PKG" && mkdir -p "$PKG/DEBIAN" "$PKG/opt/be-validation" "$PKG/usr/bin" "$PKG/usr/share/applications"
cp -r "$OUT"/. "$PKG/opt/be-validation/"
ln -s /opt/be-validation/bevalidate "$PKG/usr/bin/bevalidate"
ln -s /opt/be-validation/bevalidate-gui "$PKG/usr/bin/bevalidate-gui"
cat > "$PKG/usr/share/applications/be-validation.desktop" <<DESK
[Desktop Entry]
Type=Application
Name=BE Validation (development build)
Comment=Hardware Diagnostics & Manufacturing Quality Assurance
Exec=/opt/be-validation/bevalidate-gui
Terminal=false
Categories=System;Utility;
DESK
cat > "$PKG/DEBIAN/control" <<CTRL
Package: be-validation
Version: $DEBVER
Section: utils
Priority: optional
Architecture: amd64
Maintainer: Ballin Electric <noreply@invalid>
Depends: libc6, libx11-6, libxext6, libxft2, libxinerama1, libxcursor1, libxrender1, libxfixes3, libfontconfig1, libpango-1.0-0, libpangocairo-1.0-0, libcairo2
Recommends: smartmontools
Description: BE Validation hardware diagnostics (DEVELOPMENT BUILD, unsigned)
 Ballin Electric System Validation Suite development build. Not for
 production use. No license is granted; all rights reserved.
CTRL
# File name without '~' (GitHub release assets rename it, breaking checksums);
# the package version inside keeps Debian pre-release ordering.
DEBFILE="be-validation_${VERSION}-${LABEL#v$VERSION-}_amd64.deb"
DEBFILE="${DEBFILE/_-/_}"
dpkg-deb --root-owner-group --build "$PKG" "dist/$DEBFILE" >/dev/null
(cd dist && sha256sum "$DEBFILE" > "$DEBFILE.sha256")
echo "Built dist/$NAME.tar.gz and dist/$DEBFILE (UNSIGNED development builds)"
