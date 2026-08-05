## ADDED Requirements

### Requirement: Executable-only build script

The system SHALL provide a `frontend/build-exe.bat` script that builds only the Rust binary (`meetily.exe`) via `tauri build --no-bundle`, producing no MSI/NSIS installers.

#### Scenario: build-exe.bat produces meetily.exe only
- **WHEN** `frontend/build-exe.bat` is executed
- **THEN** it SHALL produce `meetily.exe` in `<root>/target/release/` and SHALL NOT produce any MSI or NSIS installer artifacts

#### Scenario: build-exe.bat sets up build environment
- **WHEN** `frontend/build-exe.bat` is executed
- **THEN** it SHALL configure the Visual Studio build environment, build the llama-helper sidecar, source CUDA env vars, and copy CUDA runtime DLLs before invoking `tauri build --no-bundle`

#### Scenario: build-exe.bat detects and uses CUDA
- **WHEN** `frontend/build-exe.bat` is executed on a machine with CUDA
- **THEN** it SHALL build with the `cuda` feature enabled

### Requirement: No changes to existing build scripts

The existing `build-gpu.bat` and `build-gpu.sh` scripts SHALL remain unchanged.

#### Scenario: build-gpu.bat unchanged
- **WHEN** `build-exe.bat` is created
- **THEN** `frontend/build-gpu.bat` SHALL NOT be modified
