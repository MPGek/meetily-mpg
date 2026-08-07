## 1. Cargo Toolchain Configuration

- [x] 1.1 Create `.cargo/config.toml` at repo root with `rust-lld` linker for Windows target (`[target.x86_64-pc-windows-msvc] linker = "rust-lld"`)
- [x] 1.2 Add `[profile.dev]` settings to workspace `Cargo.toml`: `debug = false`, `codegen-units = 256`, `split-debuginfo = "unpacked"`
- [x] 1.3 Add `[profile.dev.package."*"]` with `opt-level = 0` to workspace `Cargo.toml`
- [x] 1.4 Add `[build] rustc-wrapper = "sccache"` to `.cargo/config.toml`
- [x] 1.5 Verify `cargo build` succeeds with new config on Windows

## 2. Build Script Optimization

- [x] 2.1 Add `println!("cargo:rerun-if-changed=build.rs")` and `println!("cargo:rerun-if-changed=build/ffmpeg.rs")` to `frontend/src-tauri/build.rs`
- [x] 2.2 Verify build.rs does not rerun when only `src/` files change

## 3. Crate Type Cleanup

- [x] 3.1 Search codebase for references to `staticlib` output (scripts, docs, CI configs)
- [x] 3.2 Remove `staticlib` from `crate-type` in `frontend/src-tauri/Cargo.toml` (keep `cdylib` and `rlib`)
- [x] 3.3 Verify `cargo build` and `cargo tauri dev` succeed without `staticlib`

## 4. Frontend Turbopack

- [x] 4.1 Update `beforeDevCommand` in `tauri.conf.json` to `pnpm dev --turbo` (or create separate `dev:turbo` script)
- [x] 4.2 Test that Turbopack works with existing ProseMirror webpack aliases
- [x] 4.3 Add fallback script `dev:webpack` in `package.json` for `pnpm dev` (without `--turbo`)

## 5. Verification & Benchmarking

- [x] 5.1 Measure incremental build time before and after changes (clean `target/` first)
- [x] 5.2 Measure build time after `cargo clean` (tests sccache effectiveness)
- [x] 5.3 Verify Tauri dev workflow end-to-end (frontend HMR + Rust rebuild)

## 6. Documentation

- [x] 6.1 Update `docs/BUILDING.md` or `README.md` with sccache installation instructions
- [x] 6.2 Document how to temporarily re-enable debug info for debugging sessions
- [x] 6.3 Add note about Turbopack fallback if issues arise
