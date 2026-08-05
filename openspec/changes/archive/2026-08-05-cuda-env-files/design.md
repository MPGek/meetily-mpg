## Context

The Meetily project supports CUDA GPU acceleration for transcription via the `whisper-rs/cuda` Cargo feature. The CUDA toolkit (v13.3) is installed at `C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.3\` on the developer's machine.

Currently, the CUDA toolkit installer sets global environment variables:
- `CUDA_PATH` → toolkit root
- `CUDA_PATH_V13_3` → same path (version-specific, set by installer)
- `CUDA_MODULE_LOADING=LAZY` → preferred CUDA module loading mode
- Two `PATH` entries → `<root>\bin\x64` and `<root>\bin`

These globals conflict with other Python CUDA projects (PyTorch, TensorFlow, etc.) that expect their own CUDA runtime or no pre-set CUDA environment. The user wants to remove the global vars and instead set them locally before working on Meetily.

The existing scripts (`dev-gpu.sh`, `build-gpu.sh`, `dev-gpu.bat`, `build-gpu.bat`) currently rely on the global CUDA env vars being present. They also set cargo feature flags via `TAURI_GPU_FEATURE`, but do not manage CUDA-specific environment variables themselves.

## Goals / Non-Goals

**Goals:**
- Create `scripts/env-cuda.sh` that exports all CUDA env vars needed for the project
- Create `scripts/env-cuda.bat` that sets the same env vars for cmd.exe
- Modify `frontend/dev-gpu.sh` to source `../scripts/env-cuda.sh` early on Windows (where gpu scripts run via Git Bash)
- Modify `frontend/build-gpu.sh` similarly
- Modify `frontend/dev-gpu.bat` to call `..\scripts\env-cuda.bat` early
- Modify `frontend/build-gpu.bat` similarly
- All env vars point to the existing v13.3 CUDA toolkit installation

**Non-Goals:**
- No changes to the `auto-detect-gpu.js` script
- No changes to build system or Cargo features
- No detection of different CUDA versions — the files capture the current v13.3 installation
- No Linux/macOS CUDA configuration (the .sh scripts already treat CUDA as Linux-only; Windows uses Git Bash where `.sh` sourcing applies)
- This does not install CUDA or modify the CUDA toolkit installation itself

## Decisions

**1. Env files live in `scripts/` (not project root or `frontend/`)**
- `scripts/` exists for platform-specific development tooling
- Keeps root and `frontend/` clean; GPU build scripts already use `../` references to reach project root level
- Can be sourced from multiple entry points (both `frontend/` and project root)

**2. Separate `.sh` and `.bat` variants**
- The project already maintains parallel shell and batch scripts
- `.sh` files are used in Git Bash on Windows (which is the shell environment for this project)
- `.bat` files are used when running directly from cmd.exe
- Both variants must set identical environment

**3. Exact env vars to set (captured from current global state):**
- `CUDA_PATH=C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.3` — primary env var for CUDA toolkit (whisper-rs uses this on Windows)
- `CUDA_PATH_V13_3=<same>` — version-specific var (used by some build tools)
- `CUDA_MODULE_LOADING=LAZY` — lazy loading for CUDA modules (reduces startup time, avoids loading all modules)
- Add `%CUDA_PATH%\bin\x64` and `%CUDA_PATH%\bin` to `PATH` — the compiler and runtime DLLs

Note: `CUDA_HOME` is NOT needed on Windows — `whisper-rs` uses `CUDA_PATH` on this platform. The `hardware_detector.rs` checks both `CUDA_PATH` and `CUDA_HOME` but the build process only needs `CUDA_PATH`.

**4. Sourcing order: env files run early, before GPU detection**
- CUDA_PATH must be set before `auto-detect-gpu.js` runs (it checks `process.env.CUDA_PATH`)
- The env files are sourced at the top of each script, before GPU detection

**5. How scripts source the env files**
- `.sh`: `source ../scripts/env-cuda.sh` (from `frontend/` directory context)
- `.bat`: `call ..\scripts\env-cuda.bat` (from `frontend\` directory context)
- The env files should be idempotent (safe to source multiple times)

## Risks / Trade-offs

- [**Version drift**] If CUDA toolkit is updated to a different version, the paths in env-cuda.sh and env-cuda.bat must be manually updated. → Mitigation: Keep the version in the paths clearly commented at the top of each file; update as part of any CUDA toolkit upgrade.
- [**Windows path quoting**] CUDA toolkit path contains spaces (`Program Files`). → Mitigation: Both .bat and .sh files must properly quote paths. The .bat uses `"..."` throughout; the .sh uses `"..."` for all variable assignments.
- [**PATH duplication**] Sourcing the env file inside scripts that already inherit global CUDA PATH entries could double them. → Mitigation: The whole point is that global vars are removed, so this won't duplicate. If global vars somehow persist, `PATH` entries will duplicate but cause no functional harm.
- [**Forgotten sourcing**] Running cargo build directly (not via dev-gpu/build-gpu) won't have CUDA vars. → Mitigation: Document in the env files themselves that they should be sourced manually for ad-hoc builds; the scripts provide the automated path.
