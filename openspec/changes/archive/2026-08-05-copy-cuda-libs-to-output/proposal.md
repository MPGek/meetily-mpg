## Why

The Meetily CUDA build produces an executable that depends on NVIDIA CUDA runtime DLLs at load/runtime. On machines where the CUDA toolkit is not installed (or where `CUDA_PATH` no longer points to it), the app fails to start or transcribe because it cannot find `cudart`/`cuBLAS`. We need to ship only the required CUDA runtime libraries alongside the built app so it runs without failures regardless of whether the toolkit is installed.

## What Changes

- **Create** `scripts/copy-cuda-libs.*` — a script (cmd.exe batch and/or shell) that copies the required CUDA runtime DLLs from the local CUDA toolkit into the app's output folder
- **Create** `scripts/copy-cuda-libs.ps1` (or equivalent) — single source of truth listing the minimum required DLL set
- **Modify** the GPU build scripts (`frontend/build-gpu.bat`, `frontend/build-gpu.sh`) to run the copy step after a successful build
- **Modify** `frontend/scripts/tauri-auto.js` (build path) to invoke the copy step so the output folder is populated on every `tauri build`
- The DLLs are copied next to the built `meetily.exe` (in `frontend/src-tauri/target/release/` and the NSIS bundle output) so Windows locates them via the standard DLL search path

No breaking changes — only the app's output is enriched with the runtime DLLs.

## Capabilities

### New Capabilities
- `cuda-runtime-bundling`: Copy the minimum required CUDA runtime DLLs (cudart64_13.dll, cublas64_13.dll, cublasLt64_13.dll) into the app's output folder so the CUDA build runs without the toolkit installed. Complements the existing `cuda-env-files` local environment setup.

### Modified Capabilities
- *(none — no spec-level behavior changes)*

## Impact

- **Files created**: `scripts/copy-cuda-libs.ps1`, `scripts/copy-cuda-libs.bat`, `scripts/copy-cuda-libs.sh`
- **Files modified**: `frontend/build-gpu.bat`, `frontend/build-gpu.sh`, `frontend/scripts/tauri-auto.js`
- **Source of DLLs**: `%CUDA_PATH%\bin\x64` (set by `scripts/env-cuda.*`) in the local CUDA toolkit (v13.3)
- **Destination**: the app's release output folder (`frontend/src-tauri/target/release/`) and the NSIS installer content
- **Dependencies**: CUDA toolkit v13.3 (for the DLL source); the required DLL set may need updating on a toolkit upgrade
