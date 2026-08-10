## Why

Incremental Rust builds take too long during development. The project has 145 Rust source files, 848 dependencies (including heavy C/C++ compilation via whisper-rs, ort, and llama-cpp-2), a 7 GB target directory, and no build tooling configuration — no custom linker, no compilation cache, no dev profile tuning. Frontend dev uses Next.js webpack mode instead of Turbopack. Together these make the inner dev loop painfully slow, especially after branch switches or `cargo clean`.

## What Changes

- Add `.cargo/config.toml` with a faster linker (`rust-lld`) for Windows and optimized `[profile.dev]` settings (debug info reduction, codegen-units tuning, dependency opt-level)
- Add `sccache` as `rustc-wrapper` for persistent compilation caching across cleans and branch switches
- Add `cargo:rerun-if-changed` directives to `build.rs` to prevent unnecessary rebuild script reruns
- Enable Turbopack (`next dev --turbo`) for frontend dev builds
- Evaluate dropping `staticlib` from crate-type if not required (currently builds 3 output types: staticlib, cdylib, rlib)

## Capabilities

### New Capabilities
- `build-optimization`: Cargo toolchain configuration (linker, dev profile, sccache), build script rerun directives, frontend dev bundler switch, and crate-type cleanup

### Modified Capabilities
(none — no existing spec requirements are changing)

## Impact

- **Build config files**: new `.cargo/config.toml` at repo root
- **build.rs**: minor additions (rerun-if-changed directives)
- **frontend/src-tauri/Cargo.toml**: possible crate-type adjustment
- **frontend/package.json / tauri.conf.json**: beforeDevCommand may change to use `--turbo`
- **Developer workflow**: all developers need `sccache` installed; falls back gracefully if not present
- **No runtime behavior changes**: all changes are build-time only
