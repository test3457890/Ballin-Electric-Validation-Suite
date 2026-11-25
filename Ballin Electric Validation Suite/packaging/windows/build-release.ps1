# Build an unsigned Windows release folder (MSVC toolchain recommended).
# NOT YET EXECUTED ON WINDOWS. Code signing is not implemented (no keys exist).
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..\..")
cargo build --release --locked -p be-cli -p be-gui -p be-worker
$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches.Groups[1].Value
$out = "dist\be-validation-$version-windows-x64"
if (Test-Path $out) { Remove-Item -Recurse -Force $out }
New-Item -ItemType Directory -Force "$out\profiles\safety", "$out\docs" | Out-Null
Copy-Item target\release\bevalidate.exe, target\release\bevalidate-gui.exe, target\release\bevalidate-worker.exe $out
Copy-Item profiles\*.json "$out\profiles"
Copy-Item profiles\safety\EXAMPLE-not-approved.json "$out\profiles\safety"
Copy-Item README.md $out
Copy-Item "docs\Third Party Notices.md", docs\Safety.md, docs\CLI.md, "docs\Fault Codes.md", docs\Accessibility.md "$out\docs"
Compress-Archive -Path $out -DestinationPath "$out.zip" -Force
Get-FileHash "$out.zip" -Algorithm SHA256 | Format-List
Write-Host "Built $out.zip (UNSIGNED)"
