---
parent: CODEBASE_MAP.md
last_mapped: 2026-08-14T12:09:00Z
section: operations
---

> Part of [Codebase Map](CODEBASE_MAP.md)

# Operations & Deployment

## Environment Requirements

- **Node.js** ≥ 18, **Rust** toolchain **1.77+** (enforced in both `Cargo.toml` files), edition 2021.
- **Package manager**: `pnpm` preferred, `npm` accepted (scripts auto-detect).
- **CMake** ≥ 3.5 (checked/upgraded by clean build scripts).
- **Tauri CLI**: `@tauri-apps/cli ^2.1.0`; `tauri = "2.6.2"`, `tauri-build = "2.3.0"`.
- **`uv`** (Python, optional): used by `build.rs` to extract the Silero VAD model; falls back to direct GitHub download.
- **Windows**: VS Build Tools 2022 (C++ desktop workload), Windows SDK, **LLVM/Clang** at `C:\Program Files\LLVM\bin` (`LIBCLANG_PATH` required by `whisper-rs-sys`).
- **Linux**: `build-essential cmake git`; GPU dev SDKs (CUDA toolkit / ROCm / Vulkan + `libopenblas-dev`) — drivers alone are insufficient.
- **macOS**: Xcode Command Line Tools, Homebrew (`cmake node pnpm`); Metal acceleration on by default.

## Build and Deployment

### GPU feature detection & scripts

- `pnpm tauri:dev` / `pnpm tauri:build` run **`scripts/tauri-auto.js`** which reads `TAURI_GPU_FEATURE` (env override) or runs `scripts/auto-detect-gpu.js`, then calls `tauri dev|build -- --features <feat>`. Detection priority: macOS arm64→`coreml` (Intel→`metal`), NVIDIA→`cuda`, AMD ROCm→`hipblas`, Vulkan→`vulkan`, OpenBLAS→`openblas`, else CPU.
- Feature-pinned variants: `tauri:dev:cpu/cuda/vulkan/metal/coreml/openblas/hipblas` and `tauri:build:*`.
- **`frontend/build-gpu.bat` / `dev-gpu.bat` are the authoritative Windows flows**:
  1. Set `LIBCLANG_PATH`; `scripts/setup-vs-env.bat` locates & calls `vcvars64.bat` (vswhere, then VS 2022/2026 path probing; dynamic MSVC/SDK fallback).
  2. `call ..\scripts\env-cuda.bat` — sets CUDA env (see below).
  3. Detect GPU feature → `TAURI_GPU_FEATURE`.
  4. **Build the `llama-helper` sidecar** (`cargo build --release [--features <feat>]`), copy the target-triple binary to `src-tauri/binaries/llama-helper-<triple>.exe`.
  5. Run `pnpm run tauri:build` / `tauri:dev`.
- **`build-gpu.sh`/`dev-gpu.sh`** (Unix): export CUDA CMake flags on Linux (`CMAKE_CUDA_ARCHITECTURES=75`, `CMAKE_CUDA_STANDARD=17`, `CMAKE_POSITION_INDEPENDENT_CODE=ON`); `source scripts/env-cuda.sh`; **llama-helper has no `coreml` feature → remap to `metal`** on Apple Silicon; `build-gpu.sh` sets `NO_STRIP=true` for AppImage.
- **`build-gpu.ps1`/`dev-gpu.ps1`** are **Vulkan-pinned and do NOT** auto-detect GPU or build the llama-helper sidecar — not drop-in equivalents; prefer `.bat`/`.sh`.
- **`build-exe.bat`** (executable-only build): identical flow but ends with `tauri build --features cuda --no-bundle` → produces only `meetily.exe` (no MSI/NSIS). Hard-codes `--features cuda` regardless of detected GPU.
- **`scripts/env-cuda.bat` / `.sh`** (idempotent) hard-code **CUDA Toolkit v13.3**: `CUDA_ROOT=C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.3`, `CUDA_PATH`, `CUDA_PATH_V13_3`, `CUDA_MODULE_LOADING=LAZY`, prepend `<root>\bin`/`<root>\bin\x64` to PATH. Header warns: *"UPDATE THE PATH BELOW IF THE CUDA TOOLKIT VERSION CHANGES."* Does **not** set `CUDNN_LIBRARY` or `BLAS_INCLUDE_DIRS`.
- **`scripts/copy-cuda-libs.{ps1,bat,sh}`** (NEW): copy `cudart64_13.dll`, `cublas64_13.dll`, `cublasLt64_13.dll` from `$CUDA_PATH\bin\x64` into repo-root `target/release` **before** bundling — `tauri.conf.json` `bundle.resources` references `../../target/release/*_13.dll`, so these must exist or the build fails.
- **VS 2026 detection** lives in `frontend/scripts/setup-vs-env.bat`: vswhere `-latest`, then probes **VS 2026 paths before VS 2022** (BuildTools/Community/Professional/Enterprise × Program Files + (x86)), then constructs a version-agnostic `LIB`/`INCLUDE`/`PATH` from the newest MSVC toolset + Windows SDK.
- **sccache deliberately disabled**: sccache 0.17 breaks CUDA 13.3 `fatbinary` PTX and MSVC PDB-locking; `env-cuda.bat` neutralizes it via a pass-through `CMAKE_C/CXX_COMPILER_LAUNCHER`, and `rustc-wrapper` is commented out in root `.cargo/config.toml`.

### Standard commands (`frontend/package.json`)

| Command | Description |
|---------|-------------|
| `pnpm dev` | `next dev -p 3118` (frontend only) |
| `pnpm build` | `next build` |
| `pnpm tauri:dev` / `pnpm tauri:build` | Tauri dev/build with auto GPU detection |
| `pnpm tauri dev` / `pnpm tauri build` | Direct Tauri (CPU default) |

### Signed release build

`frontend/build.ps1` loads `TAURI_SIGNING_PRIVATE_KEY`/`_PASSWORD` from `.env` (via `scripts/load-env.ps1`, key from `.tauri\meetily.key`), requires them, invokes `.\build-gpu.bat`, then nulls the env vars.

## Cargo Features

Main crate (`frontend/src-tauri/Cargo.toml`):
- `default = ["platform-default"]`; `platform-default` is a **no-op marker** — real defaults come from target-specific `whisper-rs` deps.
- GPU/BLAS features all forward to `whisper-rs`: `metal`, `coreml`, `cuda`, `vulkan`, `hipblas`, `openblas`, `openmp`.
- Target-specific whisper-rs: macOS → `["raw-api","metal","coreml"]`; Windows/Linux → `["raw-api"]` (CPU; add GPU manually).
- Tauri features: `macos-private-api`, `protocol-asset`, `tray-icon`.
- Notable pins: `tauri = "2.6.2"`, `ort = "=2.0.0-rc.12"` (pre-release exact pin; rc.13+ breaks), `polyvoice = "=0.17.0"` (diarization: `onnx, download, segmentation, embedder, clusterer`), `whisper-rs = "0.16.0"`, `tauri-plugin-single-instance = "=2.3.7"` (exact), `cpal` (git rev), `ffmpeg-sidecar` (git branch `main`), `esaxx-rs` (branch `feat/dynamic-msvc-link`).

`llama-helper/Cargo.toml` (sidecar): features `metal`/`cuda`/`vulkan` forwarded to `llama-cpp-2 = "=0.1.146"` (exact-pinned). **No `coreml`/`hipblas`/`openblas`** — hence the coreml→metal remap. Release profile: `codegen-units=1`, `lto=true`, `opt-level="s"`.

Workspace members: `frontend/src-tauri`, `llama-helper`; **target dir at repo root** (`./target`).

### Incremental build speed (NEW)

- Workspace `[profile.dev]`: `debug = false`, `codegen-units = 256`, `split-debuginfo = "unpacked"`; `[profile.dev.package."*"] opt-level = 0` (fast dep builds).
- Root `.cargo/config.toml`: `linker = "rust-lld"` on `x86_64-pc-windows-msvc` for faster linking.
- sccache was the intended caching layer but is disabled (see Gotchas).

## Tauri Config (`tauri.conf.json`)

- `productName: meetily`, `version: 0.5.0`, `identifier: com.meetily.ai`.
- `frontendDist: "../out"` (static Next.js export), `devUrl: http://localhost:3118`, `beforeDevCommand: "pnpm dev"`, `beforeBuildCommand: "pnpm build"`.
- Window 1100×700, `macOSPrivateApi: true`. Tight CSP (`default-src 'self'`; `connect-src` allows localhost Ollama `11434`, `5167`, `8178`, `https://api.ollama.ai`).
- Capabilities (`main`): `fs:default`, `fs:read-all`, `fs:write-all`, `core:*:default`, `store:default`, `notification:default`, `updater:default`, `process:default`.
- Bundle targets: `deb`, `appimage`, `msi`, `nsis`, `app`, `dmg`. `externalBin`: `binaries/llama-helper`, `binaries/ffmpeg`. Resources: `templates/*.json` + CUDA runtime DLLs (`../../target/release/{cudart,cublas,cublasLt}64_13.dll`). Windows signing via `scripts/sign-windows.ps1`; macOS ad-hoc signing + hardened runtime. Updater endpoint: GitHub `meetily/meeting-minutes` latest.json.

## Gotchas

- **CUDA path hard-coded to v13.3** in `env-cuda.*`; a different toolkit version breaks CUDA builds silently.
- **VS toolchain auto-detected** by `scripts/setup-vs-env.bat` (vswhere / path probing); requires VS 2022 or 2026 with the C++ workload — no hard-coded MSVC/SDK versions.
- **`LIBCLANG_PATH`** must point at LLVM on Windows or `whisper-rs-sys` fails to parse headers.
- **llama-helper lacks `coreml`/`hipblas`/`openblas`** → remap to `metal`; others build CPU.
- **`NO_STRIP=true`** required for AppImage (set in `build-gpu.sh`).
- **Drivers ≠ acceleration**: auto-detect needs the full dev SDK (`CUDA_PATH`/`nvcc`, `ROCM_PATH`/`hipcc`, `VULKAN_SDK` + `BLAS_INCLUDE_DIRS`).
- Two diverging build paths: `tauri-auto.js` (auto) vs `tauri:build:vulkan` (pinned).
- CUDA CMake flags only set on Linux; change `CMAKE_CUDA_ARCHITECTURES` to your GPU compute capability.

## Troubleshooting

| Issue | Cause | Fix |
|-------|-------|-----|
| "CUDA toolkit not found" | Missing/version-locked toolkit | Install CUDA or update `scripts/env-cuda.*` path |
| Vulkan deps missing | Missing `BLAS_INCLUDE_DIRS` | `export VULKAN_SDK=/usr`, `export BLAS_INCLUDE_DIRS=...` |
| llama-helper binary not found | Sidecar didn't build/copy | Check `../target/{release,debug}` (workspace target at repo root) |
| No GPU acceleration | Only drivers installed, not dev SDK | Install dev SDK or force `TAURI_GPU_FEATURE=cuda` |
| AppImage symbol-strip failure | `NO_STRIP` unset | Set `NO_STRIP=true` |
| Port 3118 in use | Stale dev server | Scripts auto-kill processes on 3118 |
| Missing signing key | No `.env` | Create from `.env.example` with `TAURI_SIGNING_PRIVATE_KEY`/`_PASSWORD` |
| `whisper-rs-sys`/libclang errors | LLVM missing | Verify `C:\Program Files\LLVM\bin` + `LIBCLANG_PATH` |
| Silero VAD model not found at build | `uv`/download failed | Run the printed `uv run` command manually |

## Performance Considerations

- **`llama-cpp-2` sidecar release profile** is size-optimized (`opt-level="s"`) for faster load.
- GPU feature choice materially affects whisper.cpp speed; `auto-detect-gpu.js` chooses based on detected hardware.
- The app relies on **local** transcription/summarization (no cloud dependency) — CPU fallback exists when no GPU backend is detected.
