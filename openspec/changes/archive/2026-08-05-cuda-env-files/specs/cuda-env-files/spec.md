## ADDED Requirements

### Requirement: Local CUDA environment setup

The system SHALL provide local `env-cuda.sh` and `env-cuda.bat` files that set all CUDA environment variables required for building and running Meetily with CUDA GPU acceleration. These files MUST be placed in the `scripts/` directory at the project root.

#### Scenario: env-cuda.sh sets CUDA_PATH
- **WHEN** a user sources `scripts/env-cuda.sh` in a bash or Git Bash session
- **THEN** the environment variable `CUDA_PATH` SHALL be set to `C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.3`

#### Scenario: env-cuda.sh sets CUDA_PATH_V13_3
- **WHEN** a user sources `scripts/env-cuda.sh` in a bash or Git Bash session
- **THEN** the environment variable `CUDA_PATH_V13_3` SHALL be set to the same path as `CUDA_PATH`

#### Scenario: env-cuda.sh sets CUDA_MODULE_LOADING
- **WHEN** a user sources `scripts/env-cuda.sh` in a bash or Git Bash session
- **THEN** the environment variable `CUDA_MODULE_LOADING` SHALL be set to `LAZY`

#### Scenario: env-cuda.sh prepends CUDA bin directories to PATH
- **WHEN** a user sources `scripts/env-cuda.sh` in a bash or Git Bash session
- **THEN** the `PATH` variable SHALL have `$CUDA_PATH/bin` and `$CUDA_PATH/bin/x64` prepended

#### Scenario: env-cuda.bat sets CUDA_PATH
- **WHEN** a user runs `scripts\env-cuda.bat` in a cmd.exe session
- **THEN** the environment variable `CUDA_PATH` SHALL be set to `C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.3`

#### Scenario: env-cuda.bat sets CUDA_PATH_V13_3
- **WHEN** a user runs `scripts\env-cuda.bat` in a cmd.exe session
- **THEN** the environment variable `CUDA_PATH_V13_3` SHALL be set to the same path as `CUDA_PATH`

#### Scenario: env-cuda.bat sets CUDA_MODULE_LOADING
- **WHEN** a user runs `scripts\env-cuda.bat` in a cmd.exe session
- **THEN** the environment variable `CUDA_MODULE_LOADING` SHALL be set to `LAZY`

#### Scenario: env-cuda.bat prepends CUDA bin directories to PATH
- **WHEN** a user runs `scripts\env-cuda.bat` in a cmd.exe session
- **THEN** the `PATH` variable SHALL have `%CUDA_PATH%\bin` and `%CUDA_PATH%\bin\x64` prepended

### Requirement: GPU build scripts source the env files

The four GPU build/dev scripts SHALL source the corresponding env-cuda file before any GPU detection or build step.

#### Scenario: dev-gpu.sh sources env-cuda.sh
- **WHEN** `frontend/dev-gpu.sh` is executed on Windows via Git Bash
- **THEN** it SHALL source `../scripts/env-cuda.sh` before calling `auto-detect-gpu.js`

#### Scenario: build-gpu.sh sources env-cuda.sh
- **WHEN** `frontend/build-gpu.sh` is executed on Windows via Git Bash
- **THEN** it SHALL source `../scripts/env-cuda.sh` before calling `auto-detect-gpu.js`

#### Scenario: dev-gpu.bat sources env-cuda.bat
- **WHEN** `frontend\dev-gpu.bat` is executed in cmd.exe
- **THEN** it SHALL call `..\scripts\env-cuda.bat` before calling `auto-detect-gpu.js`

#### Scenario: build-gpu.bat sources env-cuda.bat
- **WHEN** `frontend\build-gpu.bat` is executed in cmd.exe
- **THEN** it SHALL call `..\scripts\env-cuda.bat` before calling `auto-detect-gpu.js`

### Requirement: Idempotent sourcing

The env-cuda files SHALL be safe to source multiple times without side effects (e.g., PATH doubling on each source).

#### Scenario: env-cuda.sh is safe to source repeatedly
- **WHEN** `scripts/env-cuda.sh` is sourced twice in the same shell session
- **THEN** `CUDA_PATH` SHALL be set correctly and `PATH` SHALL NOT contain duplicate CUDA entries

#### Scenario: env-cuda.bat is safe to call repeatedly
- **WHEN** `scripts\env-cuda.bat` is called twice in the same cmd.exe session
- **THEN** `CUDA_PATH` SHALL be set correctly and `PATH` SHALL NOT contain duplicate CUDA entries
