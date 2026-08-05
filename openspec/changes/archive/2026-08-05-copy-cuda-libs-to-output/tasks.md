## 1. Create CUDA copy script

- [x] 1.1 Create `scripts/copy-cuda-libs.ps1` — PowerShell script that reads `CUDA_PATH`, resolves `%CUDA_PATH%\bin\x64`, and copies `cudart64_13.dll`, `cublas64_13.dll`, `cublasLt64_13.dll` to the target output folder (creates it if missing; errors if any required DLL is absent)
- [x] 1.2 Create `scripts/copy-cuda-libs.bat` — thin batch wrapper that calls the PowerShell script (mirrors `env-cuda.bat` pattern)
- [x] 1.3 Create `scripts/copy-cuda-libs.sh` — thin shell wrapper that calls the PowerShell script (mirrors `env-cuda.sh` pattern)

## 2. Wire copy step into build flow

- [x] 2.1 Modify `frontend/scripts/tauri-auto.js` — after a successful `tauri build`, invoke the copy script targeting `src-tauri/target/release`
- [x] 2.2 Modify `frontend/build-gpu.bat` — call `..\scripts\copy-cuda-libs.bat` after the build step
- [x] 2.3 Modify `frontend/build-gpu.sh` — call `../scripts/copy-cuda-libs.sh` after the build step

## 3. Verify

- [x] 3.1 Run `scripts/copy-cuda-libs.ps1` with `CUDA_PATH` set and confirm the three DLLs appear in the target folder
- [x] 3.2 Run the copy step twice and confirm it is idempotent (no errors on re-run, files present)
- [x] 3.3 Temporarily rename a required DLL in a copy of the source and confirm the script errors instead of producing incomplete output
- [x] 3.4 Run `frontend/build-gpu.bat` and confirm the built app starts and transcribes with CUDA acceleration without the toolkit env vars set (smoke test)
