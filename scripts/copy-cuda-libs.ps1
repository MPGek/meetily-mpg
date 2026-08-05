# Copies required CUDA runtime DLLs into the app's output folder so the CUDA
# build runs without the CUDA toolkit installed.
#
# Usage:
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts/copy-cuda-libs.ps1
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts/copy-cuda-libs.ps1 -Target <dir>
#
# Idempotent: safe to run repeatedly (overwrites existing files).
# Fails loudly if CUDA_PATH is unset, the source dir is missing, or a required
# DLL is absent.
#
# UPDATE THE REQUIRED DLL LIST BELOW IF THE CUDA TOOLKIT VERSION CHANGES.
param(
    [string]$Target
)

$RequiredDlls = @(
    'cudart64_13.dll',
    'cublas64_13.dll',
    'cublasLt64_13.dll'
)

if ([string]::IsNullOrWhiteSpace($Target)) {
    $Target = Join-Path $PSScriptRoot '..\target\release'
}

$cudaPath = $env:CUDA_PATH
if ([string]::IsNullOrWhiteSpace($cudaPath)) {
    Write-Error 'CUDA_PATH is not set. Run scripts/env-cuda.bat (or source scripts/env-cuda.sh) first.'
    exit 1
}

$srcDir = Join-Path $cudaPath 'bin\x64'
if (-not (Test-Path -LiteralPath $srcDir)) {
    Write-Error "CUDA bin directory not found: $srcDir"
    exit 1
}

foreach ($dll in $RequiredDlls) {
    if (-not (Test-Path -LiteralPath (Join-Path $srcDir $dll))) {
        Write-Error "Required CUDA DLL not found: $dll (looked in $srcDir)"
        exit 1
    }
}

New-Item -ItemType Directory -Path $Target -Force | Out-Null

foreach ($dll in $RequiredDlls) {
    Copy-Item -LiteralPath (Join-Path $srcDir $dll) -Destination $Target -Force
    Write-Output "Copied $dll -> $Target"
}

Write-Output "CUDA runtime libraries copied to: $Target"
exit 0
