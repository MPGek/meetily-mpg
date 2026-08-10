## 1. Shared VS environment helper

- [x] 1.1 Create `frontend/scripts/setup-vs-env.bat` that locates `vcvars64.bat` via `vswhere.exe` (`"%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -find VC\Auxiliary\Build\vcvars64.bat`) and calls it
- [x] 1.2 Add fallback probing of VS 2026 install paths first, then VS 2022 (Build Tools/Community/Professional/Enterprise under both `Program Files` and `Program Files (x86)`) when vswhere is unavailable or returns nothing
- [x] 1.3 Add version-agnostic manual environment fallback: enumerate the newest MSVC toolset under `<install>\VC\Tools\MSVC\*` and the newest Windows SDK under `%ProgramFiles(x86)%\Windows Kits\10\{Lib,Include,bin}\*` (`for /d` + `dir /b /o-n`), and set `LIB`/`INCLUDE`/`PATH` from those — no pinned versions
- [x] 1.4 Port the `kernel32.lib` / `msvcrt.lib` verification checks into the helper, resolving them against the dynamically detected paths; fail with a clear error message and non-zero exit code when no usable VS toolchain is found

## 2. Wire the build scripts to the helper

- [x] 2.1 Replace the inline VS detection block in `frontend/build.bat` with `call "%~dp0scripts\setup-vs-env.bat"` plus an `errorlevel` check
- [x] 2.2 Same replacement in `frontend/build_backup.bat`
- [x] 2.3 Same replacement in `frontend/build-exe.bat`
- [x] 2.4 Same replacement in `frontend/build-gpu.bat`
- [x] 2.5 Same replacement in `frontend/dev-gpu.bat`
- [x] 2.6 Confirm the remaining logic of each script (LLVM `LIBCLANG_PATH`, llama-helper sidecar, CUDA copy, target triple, `tauri build`) is untouched and still receives the configured environment; verify file encodings (UTF-8, emoji output) are preserved after edits

## 3. Documentation updates

- [x] 3.1 Update `docs/BUILDING.md` (Windows prerequisites) to require "Visual Studio 2022 or 2026 — Desktop development with C++ workload"
- [x] 3.2 Update `docs/PROJECT_OVERVIEW_FULL.md` (Build Tools prerequisite section)
- [x] 3.3 Update `docs/CODEBASE_MAP_OPERATIONS.md` (Windows environment requirements line)
- [x] 3.4 Update `frontend/README.md` (Windows prerequisites and the troubleshooting note about build errors)

## 4. Verification

- [ ] 4.1 Run `frontend/build-exe.bat` on a VS 2022 machine → `meetily.exe` produced (regression check)
- [ ] 4.2 Run `frontend/build-exe.bat` on a VS 2026 machine → `meetily.exe` produced (new support)
- [ ] 4.3 Run `frontend/build-gpu.bat` on a VS 2026 machine with CUDA → GPU build succeeds with the `cuda` feature
- [x] 4.4 Run `openspec validate support-vs-2026` — delta specs pass validation
