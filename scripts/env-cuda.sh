#!/usr/bin/env bash
# Sets CUDA environment variables for Meetily development.
#
# Source this file in a bash or Git Bash session (e.g. from project root):
#   source scripts/env-cuda.sh
# Or from frontend/:  source ../scripts/env-cuda.sh
#
# Idempotent: safe to source multiple times (PATH entries are not duplicated).
#
# UPDATE THE PATHS BELOW IF THE CUDA TOOLKIT VERSION CHANGES.
CUDA_ROOT='C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.3'

export CUDA_PATH="$CUDA_ROOT"
export CUDA_PATH_V13_3="$CUDA_ROOT"
export CUDA_MODULE_LOADING="LAZY"

# Prepend CUDA bin directories to PATH only if not already present.
add_cuda_path() {
    case ":${PATH}:" in
        *":${1}:"*) ;;
        *) PATH="${1}:${PATH}" ;;
    esac
    export PATH
}

add_cuda_path "${CUDA_ROOT}/bin"
add_cuda_path "${CUDA_ROOT}/bin/x64"
