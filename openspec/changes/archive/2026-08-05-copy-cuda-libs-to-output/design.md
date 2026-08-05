## Context

Meetily supports CUDA GPU acceleration via the `whisper-rs/cuda` Cargo feature. The CUDA toolkit (v13.3) is installed at `C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.3\` on the developer's machine. The runtime DLLs used by the whisper-rs CUDA backend live in `%CUDA_PATH%\bin\x64`.

Today the CUDA build only works if the toolkit (or the global `CUDA_PATH`) is present at runtime. The completed `cuda-env-files` change removed reliance on global vars by sourcing local env files, but it does not make the built app self-contained. If the toolkit is absent or `CUDA_PATH` is not set, `meetily.exe` fails to load the CUDA runtime and transcription fails. The app should ship with the required CUDA runtime DLLs copied next to the executable.

The build pipeline is: `pnpm tauri:build` → `frontend/scripts/tauri-auto.js` → `tauri build -- --features cuda`, producing `meetily.exe` under `frontend/src-tauri/target/release/` and an NSIS installer. The GPU build scripts (`frontend/build-gpu.bat`, `frontend/build-gpu.sh`) wrap this flow and already set up the CUDA environment via `scripts/env-cuda.*`.

## Goals / Non-Goals

**Goals:**
- Copy only the minimum required CUDA runtime DLLs from `%CUDA_PATH%\bin\x64` into the app's output folder
- Make the CUDA build run without the toolkit installed (self-contained output)
- Hook the copy step into the standard build flow so it runs on every CUDA build
- Keep the required-DLL list explicit and easy to maintain

**Non-Goals:**
- No change to the CUDA build itself, Cargo features, or whisper-rs behavior
- No bundling of the full toolkit or unused CUDA libraries (no nvcc, nvrtc, cuFFT, etc.)
- No change to `auto-detect-gpu.js`
- No Linux/macOS CUDA packaging (the CUDA runtime copy is Windows-only; non-Windows builds are unchanged)
- No modifications to the CUDA toolkit installation

## Decisions

**1. Copy location: next to the built `meetily.exe`**
- whisper-rs's CUDA backend loads `cudart`/`cuBLAS` via the standard Windows DLL search path, which includes the executable's directory.
- Copied to `<root>/target/release/` (where cargo places `meetily.exe`), and referenced via `bundle.resources` in `tauri.conf.json` so the NSIS installer bundles them alongside the main executable.
- This is the simplest, most reliable location — no `PATH`/`CUDA_PATH` dependency at runtime.

**2. Minimum required DLL set (whisper-rs CUDA backend)**
- `cudart64_13.dll` — CUDA Runtime (always required)
- `cublas64_13.dll` — cuBLAS, used by ggml-cuda for matrix multiplication
- `cublasLt64_13.dll` — cuBLASLt, loaded by cuBLAS at runtime
- The NVIDIA kernel-mode driver (`nvcuda.dll`) ships with the GPU driver and is NOT copied.
- The list lives in one place (`scripts/copy-cuda-libs.ps1`) so a toolkit upgrade only updates one file.

**3. Copy engine: a PowerShell script, invoked from the existing scripts**
- PowerShell handles paths with spaces and filesystem operations reliably on Windows.
- `scripts/copy-cuda-libs.ps1` reads `CUDA_PATH`, resolves `%CUDA_PATH%\bin\x64`, and copies the three DLLs to the target directory (creating it if needed).
- `scripts/copy-cuda-libs.bat` / `scripts/copy-cuda-libs.sh` are thin wrappers so the step is callable from both `.bat` and Git Bash entry points, mirroring the `env-cuda.*` pattern.

**4. Invocation points (idempotent copy, before build)**
- `frontend/scripts/tauri-auto.js` (build path): before running `tauri build`, call the copy script targeting `<root>/target/release`. This ensures the DLLs are in place before the NSIS bundler runs, so they get included in the installer.
- `frontend/build-gpu.bat` / `frontend/build-gpu.sh`: call the wrapper before the build step (same reasoning).
- The copy is idempotent (overwrites existing files) and safe to call from multiple places.

**5. No runtime detection/selection logic**
- The required set is fixed for the v13.3 toolkit. No logic to scan or choose different CUDA versions — matching the `cuda-env-files` non-goal of version detection.

## Risks / Trade-offs

- [**Version drift**] If the toolkit is upgraded, the DLL set (filenames) must be updated in `scripts/copy-cuda-libs.ps1`. → Mitigation: single source of truth file with the version clearly commented; update as part of any CUDA toolkit upgrade.
- [**Missing DLLs break the app**] If a DLL in the required set is renamed/removed, the copy silently produces an incomplete output. → Mitigation: the script errors if any required DLL is absent from the source, so a build fails loudly instead of shipping a broken app.
- [**cuBLAS may pull extra transitive DLLs**] cuBLASLt can be self-contained in v13; if a future version needs additional DLLs, the list grows. → Mitigation: keep the list in one file and validate with a smoke test on a toolkit-less machine.
- [**License/size**] Shipping CUDA runtime DLLs adds size and NVIDIA license terms. → Mitigation: only the 3 required DLLs are shipped (small), and they are redistributable under NVIDIA's EULA for app deployment.
