## Why

The CUDA toolkit installer sets global environment variables (`CUDA_PATH`, `CUDA_PATH_V<ver>`, CUDA entries in `PATH`) that affect all tools and build scripts system-wide. This causes conflicts with other Python CUDA projects (e.g., PyTorch, TensorFlow) that expect specific or no CUDA toolkit in their environment.

We need local `env-cuda.sh` / `env-cuda.bat` files that set exactly the CUDA env vars this project needs, sourced by the build/dev scripts. This lets us remove the global CUDA environment variables entirely and only activate them per-project when working on Meetily.

## What Changes

- **Create** `scripts/env-cuda.sh` — POSIX shell script that sets CUDA environment variables for bash/Git Bash sessions
- **Create** `scripts/env-cuda.bat` — Batch script that sets CUDA environment variables for cmd.exe sessions
- **Modify** `frontend/dev-gpu.sh` to source `../scripts/env-cuda.sh` early
- **Modify** `frontend/build-gpu.sh` to source `../scripts/env-cuda.sh` early
- **Modify** `frontend/dev-gpu.bat` to call `..\scripts\env-cuda.bat` early
- **Modify** `frontend/build-gpu.bat` to call `..\scripts\env-cuda.bat` early

No breaking changes — existing behavior is preserved, only the source of CUDA env vars shifts from global to local.

## Capabilities

### New Capabilities
- `cuda-env-files`: Local CUDA environment variable files that can be sourced before build/run/dev commands. Captures exactly the vars required by this project's NVIDIA CUDA toolkit (v13.3) without polluting the global environment.

### Modified Capabilities
- *(none — no spec-level behavior changes)*

## Impact

- **Files created**: `scripts/env-cuda.sh`, `scripts/env-cuda.bat`
- **Files modified**: `frontend/dev-gpu.sh`, `frontend/build-gpu.sh`, `frontend/dev-gpu.bat`, `frontend/build-gpu.bat`
- **Environment**: Global `CUDA_PATH`, `CUDA_PATH_V13_3`, `CUDA_MODULE_LOADING`, and CUDA `PATH` entries can be removed after this change
- **Dependencies**: Uses the CUDA toolkit already installed at `C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.3\`
