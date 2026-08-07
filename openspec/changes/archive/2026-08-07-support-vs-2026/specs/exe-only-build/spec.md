## MODIFIED Requirements

### Requirement: No changes to existing build scripts

The existing `build-gpu.sh` script SHALL remain unchanged.

#### Scenario: build-gpu.sh unchanged
- **WHEN** the Windows build scripts are updated to support Visual Studio 2026
- **THEN** `frontend/build-gpu.sh` SHALL NOT be modified

**Reason**: The original requirement (from the exe-only-build change) kept both `build-gpu.bat` and `build-gpu.sh` untouched while `build-exe.bat` was created. The Windows script now needs VS 2026 support, so the restriction is relaxed for `build-gpu.bat` only; the Linux script remains frozen.
