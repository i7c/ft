## 1. Core discovery predicate

- [x] 1.1 Add `fn is_vault_root(path: &Path) -> bool` to `ft-core/src/vault.rs` returning `path.join(".obsidian").is_dir() || path.join(".ft").is_dir()` (use `.is_dir()`, not `.exists()`, for both markers)
- [x] 1.2 Replace the `canonical.join(".obsidian").exists()` checks in all three explicit-path rungs of `find_vault` (`--vault`, `FT_VAULT`, `default_vault`) with `is_vault_root(&canonical)`
- [x] 1.3 Replace `cur.join(".obsidian").exists()` in `walk_up` with `is_vault_root(cur)`

## 2. Error wording

- [x] 2.1 Update the `tried` push strings in `find_vault` from `.obsidian/`-specific to `.obsidian/ or .ft/` (`--vault`, `$FT_VAULT`, CWD walk, `default_vault` lines)
- [x] 2.2 Update the `discover()` doc comment's precedence list (rung 3 wording) and the inline marker description to reflect `.obsidian/` or `.ft/`

## 3. CLI context string

- [x] 3.1 Generalize the `anyhow::Context` string in `ft/src/cmd/common.rs::discover_vault` from "could not locate an Obsidian vault" to "could not locate a vault"

## 4. Scan exclusion clarity

- [x] 4.1 Add a code comment at `DEFAULT_IGNORED` in `ft-core/src/vault.rs` noting that dotfile directories (`.obsidian/`, `.git/`, and `.ft/`) are excluded from scans by the walker's `.hidden(true)` filter; `.ft/` is intentionally absent from this list to avoid dead config

## 5. Tests

- [x] 5.1 Add `walk_up` unit tests: `.ft/`-only ancestor found; `.ft/`-only self found; regular file named `.ft` does not qualify
- [x] 5.2 Add `find_vault`/`discover` tests covering `.ft/`-only acceptance for the `--vault` flag rung and the `FT_VAULT` rung (extend the `ENV_LOCK`-guarded env tests)
- [x] 5.3 Add a `discover` test for walk-up resolving a `.ft/`-only ancestor from a subdirectory
- [x] 5.4 Add a scan test asserting tasks under `.ft/` are not scanned (`.ft/notes.md` with task lines yields zero tasks) and `.ft` does not appear as a directory node in the graph
- [x] 5.5 Verify `error_message_lists_tried_locations` still passes (it asserts `--vault` is mentioned, not the marker name); if it asserts marker wording, refresh it for `.obsidian/ or .ft/`

## 6. Docs

- [x] 6.1 Update `docs/guide/vault-and-config.md`: intro paragraph ("the directory that contains an `.obsidian/` folder" → `.obsidian/` or `.ft/`), the precedence list rung 3 wording, and the failure block example strings
- [x] 6.2 Update `README.md` discovery one-liner (line ~151-153) to mention `.obsidian/` or `.ft/`

## 7. Build invariants

- [x] 7.1 `cargo build --release`
- [x] 7.2 `cargo test --workspace`
- [x] 7.3 `cargo clippy --workspace --tests -- -D warnings`
- [x] 7.4 `cargo fmt --check`
- [x] 7.5 `cargo run --release -q -- commands docs --check` (no keymap churn expected, but keep the invariant green)
