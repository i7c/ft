# tasks-edit-for-sync tasks

## 1. Edit fields

- [ ] 1.1 `--start`, `--status open|in-progress` (clears ✅/❌), `--depends-on IDS|none` on `EditArgs`
- [ ] 1.2 `--tags` rewrites the description's tag words (fixes the silent no-op); `--title` replaces the non-tag words
- [ ] 1.3 `--id`: refused on a task with another id, or an id held by another task

## 2. Guarded, machine-readable writes

- [ ] 2.1 `--expect <JSON>` on edit, complete, cancel → the ops' `expected` guard
- [ ] 2.2 `--format text|json` on edit, complete (`{completed, next}`), cancel
- [ ] 2.3 Exit code 3 for `LineChanged`

## 3. Bulk ids and path exclusion

- [ ] 3.1 `ops::plan_assign_ids` / `apply_assign_ids` in ft-core, with unit tests
- [ ] 3.2 `ft tasks assign-ids` (query, `--status`, `--path`, `--exclude-path`, `--dry-run`, `--format`)
- [ ] 3.3 `--exclude-path` on `ft tasks list`

## 4. Tests and invariants

- [ ] 4.1 Integration tests in `tasks_edit.rs`, `tasks_complete.rs`, `tasks_cancel.rs`, `tasks_list.rs`, `tasks_assign_ids.rs`
- [ ] 4.2 `cargo build --release`, `cargo test --workspace`, `cargo clippy --workspace --tests -- -D warnings`, `cargo fmt --check`, `commands docs --check`, `scripts/check-no-cpp.sh`
