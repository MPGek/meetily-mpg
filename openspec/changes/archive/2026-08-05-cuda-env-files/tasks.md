## 1. Create CUDA env files

- [x] 1.1 Create `scripts/env-cuda.sh` — shell file that exports CUDA_PATH, CUDA_PATH_V13_3, CUDA_MODULE_LOADING, and prepends CUDA bin directories to PATH (idempotent)
- [x] 1.2 Create `scripts/env-cuda.bat` — batch file that sets CUDA_PATH, CUDA_PATH_V13_3, CUDA_MODULE_LOADING, and prepends CUDA bin directories to PATH (idempotent)

## 2. Wire env files into build/dev scripts

- [x] 2.1 Modify `frontend/dev-gpu.sh` — source `../scripts/env-cuda.sh` early (after \#\!/env check, before GPU detection)
- [x] 2.2 Modify `frontend/build-gpu.sh` — source `../scripts/env-cuda.sh` early (after \#\!/env check, before GPU detection)
- [x] 2.3 Modify `frontend/dev-gpu.bat` — call `..\scripts\env-cuda.bat` early (after chcp/setlocal, before GPU detection)
- [x] 2.4 Modify `frontend/build-gpu.bat` — call `..\scripts\env-cuda.bat` early (after chcp/setlocal, before GPU detection)

## 3. Verify

- [x] 3.1 Source `scripts/env-cuda.sh` in Git Bash and confirm all env vars are set correctly
- [x] 3.2 Run `scripts\env-cuda.bat` in cmd.exe and confirm all env vars are set correctly
- [x] 3.3 Verify repeated sourcing/calling does not duplicate PATH entries
- [x] 3.4 Run `frontend/dev-gpu.bat` and confirm it builds with CUDA acceleration (smoke test)
