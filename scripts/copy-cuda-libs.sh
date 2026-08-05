#!/usr/bin/env bash
# Copies required CUDA runtime DLLs into the app's output folder (idempotent).
# Thin wrapper around copy-cuda-libs.ps1.
#
# Usage:
#   scripts/copy-cuda-libs.sh
#   ../scripts/copy-cuda-libs.sh   (from frontend)

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
powershell -NoProfile -ExecutionPolicy Bypass -File "$SCRIPT_DIR/copy-cuda-libs.ps1"
