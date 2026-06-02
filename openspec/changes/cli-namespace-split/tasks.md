## 1. Move `backlinks` / `links` / `journal` to `ft graph`

- [ ] 1.1 In `ft/src/cmd/graph.rs`, add `Backlinks`, `Links`, `Journal` variants to the `GraphCommand` enum
- [ ] 1.2 Lift `run_backlinks`, `run_links`, `run_journal` (and `run_links_common`) verbatim from `ft/src/cmd/notes.rs` into `ft/src/cmd/graph.rs`
- [ ] 1.3 Lift any private helpers / args structs referenced only by these three (e.g., `LinksArgs`, `JournalArgs`, `Direction` enum) into `ft/src/cmd/graph.rs`
- [ ] 1.4 Add dispatch arms in `cmd::graph::run` for the three new variants
- [ ] 1.5 Remove the `Backlinks`, `Links`, `Journal` variants and their `run_*` functions from `cmd::notes::NotesCommand` and `cmd::notes`

## 2. Fold `ft find` into `ft notes find`

- [ ] 2.1 Move the `run` function from `ft/src/cmd/find.rs` into a new private module or inline into `cmd/notes.rs` as `run_find`
- [ ] 2.2 Add `Find(FindArgs)` variant to `NotesCommand`
- [ ] 2.3 Wire `cmd::notes::run` to dispatch to `run_find`
- [ ] 2.4 Delete `ft/src/cmd/find.rs` (top-level entry point)
- [ ] 2.5 Remove `Find` variant from `Commands` in `ft/src/main.rs`

## 3. Old-path error messages

- [ ] 3.1 In `ft/src/main.rs`, before clap parses, peek at argv[1..2] for `notes backlinks`, `notes links`, `notes journal`, or top-level `find`; print the migration error and exit 2
- [ ] 3.2 Alternative if cleaner: implement these as hidden clap subcommands whose `run` immediately prints the error and exits 2 (no behaviour, just the message). Choose whichever produces a less-confusing `--help` output
- [ ] 3.3 Unit test each error path: `ft notes backlinks foo`, `ft notes links foo`, `ft notes journal foo`, `ft find foo` all produce the right exit code and message

## 4. Tests

- [ ] 4.1 Update every CLI integration test in `ft/tests/` that references the moved paths
  - [ ] 4.1.1 `notes backlinks` → `graph backlinks`
  - [ ] 4.1.2 `notes links` → `graph links`
  - [ ] 4.1.3 `notes journal` → `graph journal`
  - [ ] 4.1.4 `find` → `notes find`
- [ ] 4.2 Add tests for the four new "moved" error messages
- [ ] 4.3 Snapshot test of `ft notes --help` output reflects the new subcommand set
- [ ] 4.4 Snapshot test of `ft graph --help` output reflects the new subcommand set
- [ ] 4.5 Snapshot test of `ft --help` no longer lists `find` as a top-level command

## 5. Generated artifacts

- [ ] 5.1 Regenerate man pages via `ft man --out`
- [ ] 5.2 Regenerate shell completions (`bash`, `zsh`, `fish`) and update committed completion fixtures if any
- [ ] 5.3 Regenerate `docs/keybindings.md` (no change expected — keybindings unaffected, but the regen invariant should still pass)
- [ ] 5.4 Update the `docs/commands.md` registry section to reflect new command scopes

## 6. Docs

- [ ] 6.1 Update `README.md`: every reference to `ft notes backlinks|links|journal` and `ft find`
- [ ] 6.2 Update `docs/architecture.md`: subcommand list
- [ ] 6.3 Update `CLAUDE.md`: "New subcommand" instructions if they cite specific cmd files
- [ ] 6.4 Update `docs/timeblocks.md`, `docs/append-and-capture.md`, and any other cross-referencing docs
- [ ] 6.5 Add a CHANGELOG entry naming every renamed path

## 7. Build validation

- [ ] 7.1 `cargo build --release` — clean
- [ ] 7.2 `cargo test --workspace` — all tests pass
- [ ] 7.3 `cargo clippy --workspace --tests -- -D warnings` — clean
- [ ] 7.4 `cargo fmt --check` — clean
- [ ] 7.5 `ft completions docs --check` — clean (if commands-and-keymaps has landed by this point)
