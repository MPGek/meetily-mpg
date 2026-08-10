## Context

Five Windows batch scripts each contain a duplicated inline block (~50 lines) that sets up the Visual Studio C++ environment:

- `frontend/build.bat`, `frontend/build_backup.bat`, `frontend/build-exe.bat`, `frontend/build-gpu.bat`, `frontend/dev-gpu.bat`

The block hard-codes VS 2022 install paths (`C:\Program Files (x86)\Microsoft Visual Studio\2022\...`), MSVC toolset `14.44.35207`, and Windows SDK `10.0.22621.0`, plus a manual `LIB`/`INCLUDE`/`PATH` fallback and `kernel32.lib`/`msvcrt.lib` verification checks. On a machine with only **Visual Studio 2026** (version 18.x, installed at `C:\Program Files\Microsoft Visual Studio\2026\<Edition>`, MSVC v14.50/v14.51, Windows SDK 10.0.26100/10.0.28000) every script fails its detection chain and the build breaks.

Facts grounding the design:

- VS 2026 keeps the same `vcvars64.bat` layout: `<install>\VC\Auxiliary\Build\vcvars64.bat`.
- `vswhere.exe` ships with the VS Installer at `%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe` and discovers any installed VS product (IDE editions and Build Tools) regardless of version.
- Rust (`cc` crate) and the llama-helper sidecar build (CMake) auto-detect the MSVC toolchain from the environment once `vcvars64.bat` has run (`cl.exe`/`link.exe` on `PATH`, `LIB`, `INCLUDE`) — no Rust-side or `.cargo/config.toml` changes needed.
- PowerShell scripts delegate to the `.bat` files and need no changes; `.sh` scripts and GitHub Actions (`windows-latest` preinstalls current VS) are unaffected.
- The archived `exe-only-build` spec froze `build-gpu.bat`; the delta spec in this change relaxes that to `build-gpu.sh` only.

## Goals / Non-Goals

**Goals:**
- Build the project on a machine where only Visual Studio 2026 is installed (and where only VS 2022 is installed).
- Keep VS 2022 support fully intact; prefer the newest installed VS when both exist.
- Remove hard-coded MSVC/SDK version pins from the environment fallback.
- Single source of truth for the VS environment block instead of five drifting copies.
- Update docs so prerequisites name VS 2022 or VS 2026.

**Non-Goals:**
- No support for VS 2019 or earlier.
- No changes to Rust crates, `.cargo/config.toml`, PowerShell scripts, `.sh` scripts, or CI workflows.
- No refactoring of the rest of the build scripts (target triple detection, CUDA copy, signing, etc.).
- No changes to `docs/GPU_ACCELERATION.md` (it does not reference VS).

## Decisions

### D1: Discover the toolchain with vswhere, fall back to known paths

The shared helper locates `vcvars64.bat` with:

```bat
"%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -find VC\Auxiliary\Build\vcvars64.bat
```

- `-latest` picks the newest installation (VS 2026 when present, else VS 2022).
- `-requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64` filters to installs that actually have the x64/x86 MSVC tools (the C++ workload).
- `-products *` includes Build Tools in addition to Community/Professional/Enterprise.

If vswhere is missing or returns nothing, fall back to probing the current hard-coded path list, extended with the 2026 install dirs first (both `Program Files` and `Program Files (x86)`, Build Tools/Community/Professional/Enterprise).

*Alternatives considered:* enumerating `2022`/`2026` folders with `for /d` — rejected because it requires a code change whenever Microsoft ships a new VS year folder; vswhere is the documented, future-proof mechanism.

### D2: Extract the environment block into a shared helper script

Create `frontend/scripts/setup-vs-env.bat` containing the detection, `vcvars64.bat` invocation, dynamic fallback, and the existing verification checks. Each of the five scripts replaces its inline block with:

```bat
call "%~dp0scripts\setup-vs-env.bat"
if errorlevel 1 exit /b 1
```

*Rationale:* the five copies have already drifted (e.g. `build.bat` vs `build-exe.bat` differ in `>nul 2>&1` and echo text); one helper means VS 2026 support lands once and stays consistent. It also gives the fallback path (`scripts\` is reachable from all five scripts via `%~dp0`).

*Alternatives considered:* updating the block in place in each script — rejected due to duplication drift (the exact problem this change is fixing).

### D3: Make the manual environment fallback version-agnostic

When `vcvars64.bat` cannot be used (the existing comments note it "is not working properly" in some setups), the helper builds `LIB`/`INCLUDE`/`PATH` by enumerating the detected install's `VC\Tools\MSVC\<ver>` and the Windows SDK version dirs with `for /d` and picking the highest version (e.g. `for /d %%D in ("%VS_INSTALL%\VC\Tools\MSVC\*") do ...` and comparing/ordering via `dir /b /o-n`). The verification probes (`kernel32.lib`, `msvcrt.lib`) then run against those resolved paths.

*Rationale:* pinning 14.51/10.0.26100 would reproduce the same breakage on the next VS/SDK update; enumeration keeps both VS 2022 (14.44/10.0.22621) and VS 2026 (14.50/14.51/10.0.26100+) working without further edits.

*Alternatives considered:* pinning VS 2026 versions — rejected (same fragility as today).

### D4: Docs state "VS 2022 or VS 2026"

Update the prerequisite sections of `docs/BUILDING.md`, `docs/PROJECT_OVERVIEW_FULL.md`, `docs/CODEBASE_MAP_OPERATIONS.md`, and `frontend/README.md` from "Visual Studio Build Tools 2022" to "Visual Studio 2022 or 2026 (Desktop development with C++ workload)".

## Risks / Trade-offs

- **vswhere picks the newest install even if it is the one the user did not intend** → the `-requires` component filter excludes installs without C++ tools; if both versions have C++ tools, newest wins, which is the desired default. Users can still get a specific version by temporarily removing the other installation.
- **Newest MSVC/SDK in the fallback may be a preview toolset** → acceptable for this project (local dev builds); if it ever causes issues, the enumeration can skip `-preview`-suffixed dirs.
- **Editing `.bat` files with emoji/UTF-8 content may corrupt encoding** → preserve the existing file encoding when editing (files use UTF-8); verify by running one script after the change.
- **Behavioral change to `build-gpu.bat` conflicts with the archived `exe-only-build` spec** → resolved via the delta spec (requirement now covers `build-gpu.sh` only).
- **`build_backup.bat` is a backup artifact** → still updated for consistency per the user's "all build scripts" request; it is not executed by any pipeline.

## Migration Plan

1. Add `frontend/scripts/setup-vs-env.bat`; wire the five scripts to it.
2. Verify on a VS 2022 machine (fastest path: `frontend/build-exe.bat` → `meetily.exe`, no installer bundling).
3. Verify on a VS 2026 machine (same command).
4. Update docs.
5. Rollback is a simple `git revert` — scripts are local tooling with no runtime deployment.

## Open Questions

- None blocking. (Optional: whether `build_backup.bat` should eventually be deleted — out of scope for this change.)
