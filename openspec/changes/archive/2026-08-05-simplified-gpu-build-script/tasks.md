## 1. Create build-exe.bat

- [x] 1.1 Create `frontend/build-exe.bat` — simplified build script that sets up VS environment, builds llama-helper, sources CUDA env, copies CUDA DLLs, and runs `tauri build --features cuda --no-bundle` (no MSI/NSIS packaging)

## 2. Verify

- [x] 2.1 Run `frontend/build-exe.bat` and confirm `meetily.exe` is produced in `target/release/`
- [x] 2.2 Confirm no MSI or NSIS installer artifacts are generated
