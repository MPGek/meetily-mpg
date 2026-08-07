## Context

The project is a Tauri desktop application with:
- **Rust backend**: 145 source files, 848 dependencies in Cargo.lock, 7.1 GB target directory
- **Heavy C/C++ compilation**: whisper-rs (whisper.cpp), ort (ONNX Runtime), llama-cpp-2
- **Frontend**: Next.js 14 with static export (`output: 'export'`)
- **No build tooling configuration**: no `.cargo/config.toml`, no sccache, no custom linker, no dev profile tuning
- **Workspace**: 2 crates (main app + llama-helper)
- **Build script**: `build.rs` downloads VAD model and FFmpeg binary (cached after first run)

Current incremental builds are slow due to default MSVC linker, full debug info generation, no compilation caching, and webpack-based frontend dev builds.

## Goals / Non-Goals

**Goals:**
- Reduce incremental Rust build time by 50%+ in typical dev workflows
- Enable persistent compilation caching across `cargo clean` and branch switches
- Speed up frontend dev builds with Turbopack
- Maintain zero runtime behavior changes (build-time only)
- Ensure fallback behavior when sccache is not installed

**Non-Goals:**
- Optimize release builds (already tuned with LTO, codegen-units=1)
- Reduce dependency count (out of scope for this change)
- Migrate away from whisper-rs or ort (core functionality)
- Change CI/CD pipeline (dev-focused)

## Decisions

### 1. Linker: `rust-lld` on Windows
**Decision**: Use `rust-lld` (ships with Rust toolchain) as the default linker on Windows.

**Rationale**:
- `rust-lld` is 2-5x faster than MSVC `link.exe` for large crates
- Already bundled with Rust — no additional installation required
- Well-tested with Tauri and similar projects

**Alternatives considered**:
- `mold`: Even faster, but requires separate installation and has limited Windows support
- Keep MSVC link.exe: No, too slow for this codebase size

### 2. Compilation Cache: `sccache`
**Decision**: Configure `sccache` as `rustc-wrapper` in `.cargo/config.toml`.

**Rationale**:
- Persists compiled dependencies across `cargo clean` and branch switches
- 848 dependencies means significant cache hit potential
- Gracefully degrades if not installed (Cargo warns but continues)
- Supports local disk cache by default; can extend to S3/Redis later

**Alternatives considered**:
- `cargo-chef`: Only helps with Docker layer caching, not local dev
- No cache: Unacceptable — too much redundant compilation

**Trade-off**: Developers must install sccache (`cargo install sccache`). Documented in setup instructions.

### 3. Dev Profile: `debug = false` + `codegen-units = 256`
**Decision**: Tune `[profile.dev]` to skip debug info and maximize parallelism.

**Rationale**:
- `debug = false` eliminates debug info I/O (significant for 848 deps)
- `codegen-units = 256` increases parallel compilation (default is 16)
- Stack traces still work via panic hooks; only symbolic debug info is omitted

**Alternatives considered**:
- `debug = "line-tables-only"`: Middle ground, but `false` is simpler and faster
- Keep default debug info: Too slow for this codebase

**Trade-off**: Losing symbolic debug info makes some debugger workflows harder. Acceptable for this project's dev style (log-driven + occasional debugger).

### 4. Frontend Dev: Turbopack
**Decision**: Enable Turbopack for `next dev` via `beforeDevCommand` or package.json script.

**Rationale**:
- Turbopack is 10-20x faster than webpack for initial dev startup
- Incremental updates are near-instant
- Next.js 14 supports Turbopack (stable enough for dev)

**Alternatives considered**:
- Keep webpack: Too slow for this many components and aliases
- Vite migration: Too invasive (requires rewiring Next.js-specific features)

**Trade-off**: Turbopack is newer and may have edge cases. If issues arise, can revert to webpack per-script.

### 5. Crate Type: Drop `staticlib`
**Decision**: Remove `staticlib` from `crate-type` if not required.

**Rationale**:
- Tauri 2 uses `cdylib` for the final binary
- `staticlib` is only needed for embedding Rust in C/C++ (not the case here)
- Each crate type adds a full link pass

**Alternatives considered**:
- Keep all three: Unnecessary overhead
- Only `rlib`: Breaks Tauri build (needs `cdylib`)

**Trade-off**: If any external tooling depends on the staticlib, it will break. Verify with `grep` for references before removing.

### 6. Build Script: Explicit Rerun Directives
**Decision**: Add `cargo:rerun-if-changed` to `build.rs` for itself and `build/ffmpeg.rs`.

**Rationale**:
- Default behavior reruns build.rs on any crate file change
- Explicit directives prevent unnecessary reruns
- FFmpeg/VAD downloads are already cached, but file checks still run

**Alternatives considered**:
- No change: Minor overhead, but worth fixing for correctness

## Risks / Trade-offs

| Risk | Mitigation |
|------|------------|
| `rust-lld` incompatibility with specific Windows SDK versions | Test on CI; fallback to MSVC link.exe if issues arise |
| sccache cache corruption | Document cache clear command (`sccache --stop-server` + delete cache dir) |
| Turbopack edge cases with ProseMirror aliases | Monitor dev logs; revert to webpack script if needed |
| Dropping `staticlib` breaks external tooling | Verify no references before removing; keep in separate branch for testing |
| `debug = false` makes debugging harder | Document how to temporarily re-enable for debugging sessions |

## Migration Plan

1. Create `.cargo/config.toml` with linker and profile settings
2. Test build on Windows (primary dev platform)
3. Add sccache to dev setup documentation
4. Update `beforeDevCommand` or package.json to use `--turbo`
5. Verify Tauri dev build works end-to-end
6. Evaluate `staticlib` removal in separate PR after confirming no dependencies

**Rollback**: All changes are additive except crate-type. Revert by deleting `.cargo/config.toml` and restoring original `beforeDevCommand`.

## Open Questions

- Does any external tooling depend on the `staticlib` output? (Need to grep codebase and docs)
- Should we provide a `dev-debug` profile that re-enables debug info for debugging sessions?
- Are there any Turbopack incompatibilities with the ProseMirror webpack aliases?
