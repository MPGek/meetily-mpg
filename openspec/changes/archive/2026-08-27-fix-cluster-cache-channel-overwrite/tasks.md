## 1. Fix the DELETE query in write_cluster_cache

- [x] 1.1 Open `frontend/src-tauri/src/database/repositories/speaker.rs`
- [x] 1.2 Locate the `write_cluster_cache` function (around line 284)
- [x] 1.3 Find the DELETE statement: `DELETE FROM speaker_embeddings WHERE meeting_id = ? AND cluster_label = ?`
- [x] 1.4 Add the `channel` parameter to the DELETE query: `DELETE FROM speaker_embeddings WHERE meeting_id = ? AND cluster_label = ? AND channel = ?`
- [x] 1.5 Add `.bind(channel)` to the query chain after the existing binds

## 2. Verify the fix

- [x] 2.1 Run the existing tests to ensure no regressions: `cargo test --package app-lib`
- [x] 2.2 Check that the function signature and callers remain unchanged
- [x] 2.3 Verify the DELETE query now includes the channel filter in the compiled code

## 3. Documentation

- [x] 3.1 Add a comment explaining that the DELETE is channel-scoped to preserve cross-channel cache independence
- [x] 3.2 Update any inline documentation if needed
