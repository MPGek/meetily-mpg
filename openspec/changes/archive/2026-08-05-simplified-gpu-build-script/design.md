## Context

Meetily has a full build pipeline (`build-gpu.bat`, 275 lines) that produces production installers (MSI, NSIS) via `tauri build`. This involves setting up VS environment, building the llama-helper sidecar, copying CUDA DLLs, detecting GPU features, and running `tauri build` which compiles the binary AND generates installer packages.

For fast iteration, CI, or when only the executable is needed, the full build is overkill. Tauri CLI provides `--no-bundle` to skip the bundling step, producing only `meetily.exe` in `target/release/`.

## Goals / Non-Goals

**Goals:**
- Create a simplified `build-exe.bat` that produces `meetily.exe` only, with no MSI/NSIS packaging
- Keep essential setup: VS environment, llama-helper sidecar build, CUDA env, GPU detection
- Fast feedback loop for dev iteration

**Non-Goals:**
- No changes to existing `build-gpu.bat` or `build-gpu.sh`
- No cross-platform support (Windows-only `.bat` variant)
- No changes to Tauri config or bundling behavior

## Decisions

**1. Single file: `frontend/build-exe.bat`**
- Mirrors the structure of `build-gpu.bat` but omits installer-specific steps
- Uses `tauri build --no-bundle` instead of `pnpm run tauri:build` (which goes through `tauri-auto.js` and would also bundle)

**2. Retained steps (copied from `build-gpu.bat`)**
- VS environment setup (vcvars64)
- llama-helper sidecar build with GPU features
- Target triple detection and binary copy to `src-tauri/binaries/`
- CUDA env sourcing and DLL copy (so the exe runs with CUDA)

**3. Omitted steps**
- `tauri build` full bundling (MSI/NSIS)
- Post-build installer generation

**4. Direct `tauri build --no-bundle` invocation**
- Instead of `pnpm run tauri:build` (which calls `tauri-auto.js`), the script invokes `tauri build --features cuda --no-bundle` directly
- This is simpler and faster — no auto-detect wrapper needed since the script is CUDA-specific

## Risks / Trade-offs

- [**Duplication**] `build-exe.bat` shares ~80% of its setup with `build-gpu.bat`. → Mitigation: Accept duplication for simplicity; refactoring into shared functions adds complexity for a script that's meant to be standalone.
- [**Feature parity**] `build-exe.bat` hardcodes CUDA feature. → Mitigation: The script is explicitly for CUDA GPU builds; non-CUDA builds use other scripts.
