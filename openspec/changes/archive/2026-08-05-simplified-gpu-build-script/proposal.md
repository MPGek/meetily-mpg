## Why

The existing `build-gpu.bat` (275 lines) performs a full production build: Visual Studio setup, llama-helper sidecar build, target triple detection, binary copy, CUDA DLL copy, and `tauri build` with MSI/NSIS installer generation. For quick iteration or CI where only the executable is needed (no installer), this is slow and produces unnecessary artifacts. A simpler "exe-only" build script skips bundling entirely, producing just `meetily.exe` in `target/release/`.

## What Changes

- **Create** `frontend/build-exe.bat` — simplified Windows batch script that builds only the Rust binary via `tauri build --no-bundle`, skipping MSI/NSIS installer generation
- The script retains the essentials: VS environment setup, llama-helper sidecar build, CUDA env sourcing, GPU detection, and binary copy — but omits installer generation
- No changes to existing scripts; the full `build-gpu.bat` remains for production builds

## Capabilities

### New Capabilities
- `exe-only-build`: A fast, simplified build script that produces `meetily.exe` without packaging installers. Useful for dev iteration, CI, and ad-hoc builds.

### Modified Capabilities
- *(none)*

## Impact

- **Files created**: `frontend/build-exe.bat`
- **No breaking changes**: `build-gpu.bat` and `build-gpu.sh` remain unchanged for full builds
- **Dependencies**: Same as `build-gpu.bat` (Visual Studio Build Tools, Rust, LLVM, CUDA toolkit for GPU builds)
