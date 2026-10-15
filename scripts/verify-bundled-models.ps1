# verify-bundled-models.ps1
# Asserts that enhanced diarization models are bundled for release builds.
# Checks frontend/src-tauri/models/*.onnx existence and size >1KB, and optional resource dir check.

$ErrorActionPreference = "Stop"

$modelsDir = Join-Path $PSScriptRoot "..\frontend\src-tauri\models"
$seg = Join-Path $modelsDir "segmentation-3.0.onnx"
$emb = Join-Path $modelsDir "titanet_large.onnx"

function Assert-Model($path, $label) {
    if (-not (Test-Path $path)) {
        Write-Error "Missing $label at $path. Diarization will fail at runtime (no bundled models). Rebuild with network."
        exit 1
    }
    $size = (Get-Item $path).Length
    if ($size -le 1024) {
        Write-Error "$label at $path is too small ($size bytes). Expected >1KB."
        exit 1
    }
    Write-Host "✅ $label found: $path ($size bytes)"
}

Assert-Model $seg "segmentation-3.0.onnx"
Assert-Model $emb "titanet_large.onnx"

# Optional: check bundled resource dir if built artifacts exist (target/release/bundle)
$bundleModels = Get-ChildItem -Path "target" -Recurse -Filter "segmentation-3.0.onnx" -ErrorAction SilentlyContinue | Select-Object -First 1
if ($bundleModels) {
    Write-Host "✅ Bundled resource check: found $($bundleModels.FullName)"
}

Write-Host "✅ All enhanced models verified for bundling"
