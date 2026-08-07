## ADDED Requirements

### Requirement: Visual Studio toolchain discovery

The Windows build scripts SHALL locate and configure the Visual Studio C++ build environment from any installed Visual Studio 2022 or Visual Studio 2026 installation (IDE editions and Build Tools), preferring the newest available installation.

#### Scenario: Build on a machine with Visual Studio 2026 only
- **WHEN** a Windows build script is executed on a machine where only Visual Studio 2026 is installed
- **THEN** the script SHALL configure the C++ environment from the VS 2026 installation and the build SHALL succeed

#### Scenario: Build on a machine with Visual Studio 2022 only
- **WHEN** a Windows build script is executed on a machine where only Visual Studio 2022 is installed
- **THEN** the script SHALL configure the C++ environment from the VS 2022 installation and the build SHALL succeed

#### Scenario: Discovery via vswhere
- **WHEN** `vswhere.exe` is available at `%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe`
- **THEN** the script SHALL use vswhere to locate `vcvars64.bat` in the newest installation that provides the MSVC x64/x86 tools (component `Microsoft.VisualStudio.Component.VC.Tools.x86.x64`)

#### Scenario: Fallback when vswhere is unavailable
- **WHEN** `vswhere.exe` is not available
- **THEN** the script SHALL fall back to probing the standard Visual Studio 2026 and 2022 install paths (Build Tools, Community, Professional, Enterprise under both `Program Files` and `Program Files (x86)`)

### Requirement: Version-agnostic manual environment fallback

When `vcvars64.bat` cannot be used, the Windows build scripts SHALL construct `LIB`, `INCLUDE`, and `PATH` from the newest installed MSVC toolset and the newest installed Windows SDK within the detected Visual Studio installation, rather than from pinned version numbers.

#### Scenario: VS 2026 with newer MSVC and SDK
- **WHEN** the detected installation is VS 2026 containing MSVC v14.51 and Windows SDK 10.0.26100
- **THEN** the fallback environment SHALL reference the v14.51 toolset and the 10.0.26100 SDK directories

#### Scenario: VS 2022 with older MSVC and SDK
- **WHEN** the detected installation is VS 2022 containing MSVC 14.44 and Windows SDK 10.0.22621.0
- **THEN** the fallback environment SHALL reference the 14.44 toolset and the 10.0.22621.0 SDK directories

### Requirement: All Windows build scripts support VS 2026

The scripts `frontend/build.bat`, `frontend/build_backup.bat`, `frontend/build-exe.bat`, `frontend/build-gpu.bat`, and `frontend/dev-gpu.bat` SHALL perform the Visual Studio toolchain detection described above before invoking any build command.

#### Scenario: build.bat and build_backup.bat on VS 2026
- **WHEN** `frontend/build.bat` or `frontend/build_backup.bat` is executed on a VS 2026 machine
- **THEN** the script SHALL configure the VS 2026 environment and build the application successfully

#### Scenario: build-exe.bat on VS 2026
- **WHEN** `frontend/build-exe.bat` is executed on a VS 2026 machine
- **THEN** it SHALL configure the VS 2026 environment and build `meetily.exe` successfully

#### Scenario: build-gpu.bat and dev-gpu.bat on VS 2026
- **WHEN** `frontend/build-gpu.bat` or `frontend/dev-gpu.bat` is executed on a VS 2026 machine
- **THEN** the script SHALL configure the VS 2026 environment and build the GPU-enabled application successfully
