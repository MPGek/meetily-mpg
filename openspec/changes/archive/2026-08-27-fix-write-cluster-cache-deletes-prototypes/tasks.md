## 1. Fix write_cluster_cache DELETE query

- [x] 1.1 Update `write_cluster_cache` in `frontend/src-tauri/src/database/repositories/speaker.rs` at line 284 to add `AND speaker_id IS NULL` to the DELETE query, so it only removes unassigned cache rows and preserves enrolled prototypes

## 2. Regression test

- [x] 2.1 Create a test that verifies enrolled prototypes (rows with `speaker_id IS NOT NULL`) survive a subsequent `write_cluster_cache` call on the same cluster
