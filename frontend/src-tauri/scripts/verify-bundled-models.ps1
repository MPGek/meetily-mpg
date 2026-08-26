# verify-bundled-models.ps1 (tauri-local copy)
# Delegates to repo root scripts/verify-bundled-models.ps1

$root = Join-Path $PSScriptRoot "..\..\scripts\verify-bundled-models.ps1"
& $root
exit $LASTEXITCODE
