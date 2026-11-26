# Package an UNSIGNED Windows x64 portable ZIP of BE Validation.
# Used by .github/workflows/release.yml (MSVC toolchain, static CRT).
# Code signing is NOT performed: no signing credentials exist.
param(
    [switch]$NoBuild,
    [string]$Label = ""
)
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..\..")
if ($Label -cnotmatch '^v') { $Label = "" }
if ($Label -ne "" -and $Label -cnotmatch '\Av[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9]+([.-][a-zA-Z0-9]+)*)?\z') {
    throw "Invalid release label"
}
if (-not $NoBuild) {
    $env:RUSTFLAGS = "-C target-feature=+crt-static"
    cargo build --release --locked -p be-cli -p be-gui -p be-worker
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
}
$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches.Groups[1].Value
if ($Label -eq "") { $Label = "v$version-dev" }
$name = "BE-Validation-$Label-windows-x64-portable"
$out = "dist\$name"
if (Test-Path $out) { Remove-Item -Recurse -Force $out }
New-Item -ItemType Directory -Force "$out\profiles\safety", "$out\docs" | Out-Null
Copy-Item target\release\bevalidate.exe, target\release\bevalidate-gui.exe, target\release\bevalidate-worker.exe $out
Copy-Item profiles\*.json "$out\profiles"
Copy-Item profiles\safety\EXAMPLE-not-approved.json "$out\profiles\safety"
Copy-Item "packaging\windows\README-WINDOWS.txt" "$out\README.txt"
Copy-Item "docs\Windows Development Build.md", "docs\Physical Hardware Testing Guide.md", "docs\Known Limitations and Troubleshooting.md", "docs\Third Party Notices.md", "docs\Safety.md", "docs\CLI.md", "docs\Fault Codes.md", "docs\Accessibility.md" "$out\docs"
"BE Validation $Label`r`nDEVELOPMENT BUILD - NOT FOR PRODUCTION USE - UNSIGNED`r`nBuilt: $((Get-Date).ToUniversalTime().ToString('u'))`r`nCommit: $env:GITHUB_SHA`r`nToolchain: MSVC x86_64-pc-windows-msvc, static CRT" | Set-Content "$out\BUILD-INFO.txt"
$hashes = Get-ChildItem $out -Recurse -File | ForEach-Object { "{0}  {1}" -f (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower(), $_.FullName.Substring((Resolve-Path $out).Path.Length + 1) }
$hashes | Set-Content "$out\SHA256SUMS-contents.txt"
$zip = "dist\$name.zip"
if (Test-Path $zip) { Remove-Item $zip }
Compress-Archive -Path $out -DestinationPath $zip
$h = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
"$h  $name.zip" | Set-Content "dist\$name.zip.sha256" -NoNewline
Write-Host "Built $zip  sha256=$h  (UNSIGNED development build)"
