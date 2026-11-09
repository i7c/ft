# tasks-edit-for-sync tasks

## 1. Edit fields

- [x] 1.1 `--start`, `--status open|in-progress` (clears ✅/❌), `--depends-on IDS|none` on `EditArgs`
- [x] 1.2 `--tags` rewrites the description's tag words (fixes the silent no-op); `--title` replaces the non-tag words
- [x] 1.3 `--id`: refused on a task with another id, or an id held by another task

## 2. Guarded, machine-readable writes

- [x] 2.1 `--expect <JSON>` on edit, complete, cancel → the ops' `expected` guard
- [x] 2.2 `--format text|json` on edit, complete (`{completed, next}`), cancel
- [x] 2.3 Exit code 3 for `LineChanged`

## 3. Bulk ids and path exclusion

- [x] 3.1 `ops::plan_assign_ids` / `apply_assign_ids` in ft-core, with unit tests
- [x] 3.2 `ft tasks assign-ids` (query, `--status`, `--path`, `--exclude-path`, `--dry-run`, `--format`)
- [x] 3.3 `--exclude-path` on `ft tasks list`

## 4. Tests and invariants

- [x] 4.1 Integration tests, in one file for the change: `ft/tests/tasks_edit_for_sync.rs`
- [x] 4.2 `cargo build --release`, `cargo test --workspace`, `cargo clippy --workspace --tests -- -D warnings`, `cargo fmt --check`, `commands docs --check`, `scripts/check-no-cpp.sh`
