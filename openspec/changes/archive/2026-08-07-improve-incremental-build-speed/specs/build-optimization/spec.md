## ADDED Requirements

### Requirement: Faster linker for Rust builds
The project SHALL configure `rust-lld` as the default linker for Windows targets via `.cargo/config.toml`.

#### Scenario: Incremental Rust build uses rust-lld
- **WHEN** a developer runs `cargo build` on Windows
- **THEN** the build uses `rust-lld` as the linker instead of the default MSVC `link.exe`

#### Scenario: rust-lld is available without extra installation
- **WHEN** a developer has the Rust toolchain installed
- **THEN** `rust-lld` is available as it ships with the Rust toolchain

### Requirement: Compilation caching with sccache
The project SHALL configure `sccache` as the `rustc-wrapper` in `.cargo/config.toml` to cache compiled artifacts.

#### Scenario: sccache caches dependency compilation
- **WHEN** a developer runs `cargo build` after `sccache` is installed
- **THEN** compiled dependencies are cached and reused on subsequent builds

#### Scenario: Build succeeds without sccache installed
- **WHEN** a developer does not have `sccache` installed
- **THEN** `cargo build` still succeeds (with a warning about missing wrapper)

### Requirement: Optimized dev profile
The project SHALL configure `[profile.dev]` in the workspace `Cargo.toml` to disable debug info and increase codegen parallelism.

#### Scenario: Dev builds skip debug info generation
- **WHEN** a developer runs `cargo build` (debug mode)
- **THEN** debug info is not generated (`debug = false`), reducing I/O

#### Scenario: Dev builds use maximum parallelism
- **WHEN** a developer runs `cargo build` (debug mode)
- **THEN** `codegen-units` is set to 256 for maximum compilation parallelism

#### Scenario: Dependencies are not optimized in dev mode
- **WHEN** a developer runs `cargo build` (debug mode)
- **THEN** all dependencies are compiled with `opt-level = 0` via `[profile.dev.package."*"]`

### Requirement: Turbopack for frontend dev
The frontend dev server SHALL use Turbopack (`next dev --turbo`) for faster startup and HMR.

#### Scenario: Frontend dev uses Turbopack
- **WHEN** a developer runs the Tauri dev command
- **THEN** the frontend dev server starts with Turbopack enabled

#### Scenario: Fallback to webpack is possible
- **WHEN** Turbopack causes issues
- **THEN** developers can use an alternative script to run with webpack

### Requirement: Build script rerun directives
The `build.rs` script SHALL include explicit `cargo:rerun-if-changed` directives to prevent unnecessary reruns.

#### Scenario: build.rs only reruns when its inputs change
- **WHEN** a source file in `src/` is modified
- **THEN** `build.rs` does NOT rerun (only `cargo:rerun-if-changed=build.rs` and `cargo:rerun-if-changed=build/ffmpeg.rs` trigger it)

### Requirement: Crate type cleanup
The `crate-type` in `frontend/src-tauri/Cargo.toml` SHALL be evaluated and `staticlib` removed if not required.

#### Scenario: Only necessary crate types are built
- **WHEN** the project is built
- **THEN** only `cdylib` and `rlib` are produced (if `staticlib` is confirmed unnecessary)
