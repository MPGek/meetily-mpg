//! Fails when `tao` is no longer resolved from the vendored, patched copy in `vendor/tao`.
//!
//! Cargo only warns about an unused `[patch.crates-io]` entry, so a `cargo update` that moves
//! `tao` past the vendored version would silently bring back the 0xC000001D crash in
//! `tauri_runtime_wry::Context::clone`. See `vendor/tao/MEETILY_PATCH.md`.

const WORKSPACE_LOCK: &str = include_str!("../../../Cargo.lock");

/// Returns `Err` with an explanation unless every `tao` package in `lock` is a path package
/// (path packages have no `source = ` line in `Cargo.lock`).
fn check_tao_is_vendored(lock: &str) -> Result<(), String> {
    let tao_blocks: Vec<&str> = lock
        .split("[[package]]")
        .filter(|block| block.lines().any(|l| l.trim() == r#"name = "tao""#))
        .collect();

    if tao_blocks.is_empty() {
        return Err("no `tao` package found in Cargo.lock; if tauri no longer depends on tao, \
                    remove the patch per vendor/tao/MEETILY_PATCH.md"
            .to_string());
    }

    for block in tao_blocks {
        if let Some(source) = block.lines().find(|l| l.trim_start().starts_with("source = ")) {
            return Err(format!(
                "`tao` resolves from `{}` instead of vendor/tao, so the atomic event-loop runner \
                 refcount patch is NOT applied. Re-apply or remove it as described in \
                 vendor/tao/MEETILY_PATCH.md",
                source.trim()
            ));
        }
    }
    Ok(())
}

#[test]
fn tao_resolves_from_vendored_patch() {
    if let Err(msg) = check_tao_is_vendored(WORKSPACE_LOCK) {
        panic!("{msg}");
    }
}

#[test]
fn registry_tao_is_rejected() {
    // The checked-out lockfile may use CRLF line endings.
    let lf_lock = WORKSPACE_LOCK.replace("\r\n", "\n");
    let lock = lf_lock.replacen(
        "name = \"tao\"\n",
        "name = \"tao\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n",
        1,
    );
    assert!(lock != lf_lock, "test fixture did not find the tao entry to rewrite");
    let err = check_tao_is_vendored(&lock).expect_err("registry tao must be rejected");
    assert!(err.contains("vendor/tao/MEETILY_PATCH.md"), "{err}");
}
