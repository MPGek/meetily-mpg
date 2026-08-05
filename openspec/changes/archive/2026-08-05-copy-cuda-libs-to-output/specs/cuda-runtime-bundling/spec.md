## ADDED Requirements

### Requirement: Copy required CUDA runtime DLLs to output

The CUDA build pipeline SHALL copy the minimum required CUDA runtime DLLs from the local CUDA toolkit's `bin\x64` directory into the app's output folder, so the built `meetily.exe` runs without the CUDA toolkit installed. The required DLLs SHALL be `cudart64_13.dll`, `cublas64_13.dll`, and `cublasLt64_13.dll`.

#### Scenario: Copy DLLs on successful build
- **WHEN** a CUDA build completes successfully with `CUDA_PATH` set to the v13.3 toolkit
- **THEN** the files `cudart64_13.dll`, `cublas64_13.dll`, and `cublasLt64_13.dll` SHALL be present in the app's output folder

#### Scenario: Source from CUDA_PATH bin\x64
- **WHEN** the copy step runs with `CUDA_PATH` set to the CUDA toolkit root
- **THEN** the DLLs SHALL be copied from `%CUDA_PATH%\bin\x64`

#### Scenario: Error when a required DLL is missing
- **WHEN** a required DLL is not present in the source `bin\x64` directory
- **THEN** the copy step SHALL report an error and not silently produce an incomplete output

### Requirement: Required DLL list is centralized

The set of required CUDA runtime DLLs SHALL be defined in a single source-of-truth file so a CUDA toolkit upgrade updates only one place.

#### Scenario: DLL list in one file
- **WHEN** the list of required CUDA DLLs is updated
- **THEN** only the single source-of-truth file SHALL need to be edited

### Requirement: Copy step is invoked by the build flow

The copy step SHALL run automatically as part of the CUDA build and dev GPU scripts so the output folder is populated without manual action.

#### Scenario: Copy after tauri build
- **WHEN** the standard CUDA build flow (`tauri build` via `tauri-auto.js` or `build-gpu.*`) completes
- **THEN** the required CUDA DLLs SHALL be copied into the app's release output folder

#### Scenario: Copy is idempotent
- **WHEN** the copy step is run more than once
- **THEN** the required CUDA DLLs SHALL be present and no error SHALL occur on re-runs
