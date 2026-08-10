## Why

All Windows build scripts (`build.bat`, `build-gpu.bat`, `build-exe.bat`, `dev-gpu.bat`, `build_backup.bat`) hard-code Visual Studio 2022 install paths, MSVC toolset `14.44.35207`, and Windows SDK `10.0.22621.0`. On a machine with only Visual Studio 2026 installed (version 18.x, MSVC v14.50+, Windows SDK 10.0.26100/10.0.28000) every `.bat` script falls through its detection chain, fails to configure the C++ environment, and the build breaks. VS 2026 is now the current release (18.8 as of July 2026), so the scripts must work with both VS 2022 and VS 2026.

## What Changes

- Replace the hard-coded VS 2022 detection chains in all five `.bat` scripts with version-agnostic toolchain discovery: locate `vcvars64.bat` via **vswhere** (covers any VS 2022/2026 edition and Build Tools), with a fallback to known install paths (2026 first, then 2022) when vswhere is unavailable.
- Make the manual `LIB`/`INCLUDE`/`PATH` fallback (used when `vcvars64.bat` is not working) version-agnostic: enumerate the newest installed MSVC toolset and Windows SDK in the detected install directory instead of pinning `14.44.35207` / `10.0.22621.0`.
- Keep VS 2022 support fully intact — this is additive and backward compatible.
- Update documentation (`docs/BUILDING.md`, `docs/PROJECT_OVERVIEW_FULL.md`, `docs/CODEBASE_MAP_OPERATIONS.md`, `frontend/README.md`) to state that VS 2022 **or** VS 2026 with the "Desktop development with C++" workload is required.
- The existing `exe-only-build` spec requirement "No changes to existing build scripts" is superseded for `build-gpu.bat` (it now receives VS 2026 support); `build-gpu.sh` remains untouched.

## Capabilities

### New Capabilities
- `vs-toolchain-detection`: Windows build scripts detect and configure a Visual Studio 2022 or Visual Studio 2026 C++ toolchain (vcvars64.bat via vswhere with path fallback, dynamic MSVC/SDK fallback environment) before invoking any Rust/Tauri build.

### Modified Capabilities
- `exe-only-build`: the requirement "No changes to existing build scripts" is replaced — `build-gpu.bat` SHALL be updated to support VS 2026, while `build-gpu.sh` stays unchanged.

## Impact

- **Scripts (Windows, `.bat`)**: `frontend/build.bat`, `frontend/build_backup.bat`, `frontend/build-exe.bat`, `frontend/build-gpu.bat`, `frontend/dev-gpu.bat`.
- **Docs**: `docs/BUILDING.md`, `docs/PROJECT_OVERVIEW_FULL.md`, `docs/CODEBASE_MAP_OPERATIONS.md`, `frontend/README.md`.
- **Not affected**: Rust crates/`Cargo.toml` (MSVC is auto-detected once the env is set), `.cargo/config.toml` (macOS-only), PowerShell scripts (they delegate to the `.bat` files), `.sh` scripts (Linux), GitHub Actions (runs on `windows-latest` which preinstalls current VS tooling, no pinned VS versions).
- **Existing spec conflict**: `openspec/specs/exe-only-build/spec.md` "No changes to existing build scripts" requirement must be amended via delta spec.
