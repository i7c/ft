## 1. ft-core: extend `score_related` to ghosts

- [ ] 1.1 In `ft-core/src/related.rs::score_related`, replace the `NodeKind::Note(n) => …; _ => return Ok(Vec::new())` early-return with ghost handling mirroring `journal::build_journal` (`NodeKind::Ghost(_) => note_path = None, aliases = []`), so the co-occurrence walk runs against the ghost with an empty alias set.
- [ ] 1.2 Verify `already_in_related` is uniformly `false` for ghost-target results (no alias set is read).
- [ ] 1.3 Add unit tests in `related.rs`: ghost target produces scored concepts; ghost target skips alias resolution (no `already_in_related == true` rows); ghost self-exclusion still holds.
- [ ] 1.4 `cargo test -p ft-core related` green.

## 2. ft-core: shared note-or-ghost resolver

- [ ] 2.1 Rename `ft/src/cmd/notes.rs::resolve_journal_target` → `resolve_note_or_ghost` (the `[[]]`-aware path → title → fuzzy → ghost-fallback resolver).
- [ ] 2.2 Update the single `run_journal` call site to the new name; confirm behavior-preserving.
- [ ] 2.3 `cargo build --workspace` green; `ft/tests/notes_journal.rs` + `ft/tests/journal_multi_link_cli.rs` still pass.

## 3. ft: `ft notes related` output module

- [ ] 3.1 Create `ft/src/output/related.rs` with `RelatedRow { title, score, already_in_related, target: LinkRowTarget }` (reuse `LinkRowTarget` from `output/links.rs` for `Resolved{path}` / `Unresolved{raw}`).
- [ ] 3.2 Implement `render_table` (with `--no-color`/`NO_COLOR`/non-TTY auto-off + distinct marking for `already_in_related` rows), `render_json`, `render_ndjson`, `render_markdown` — structured like `output/links.rs`.
- [ ] 3.3 Register the module in `ft/src/output/mod.rs`.

## 4. ft: `ft notes related` subcommand

- [ ] 4.1 Add `NotesCommand::Related(RelatedArgs)` + `RelatedArgs` (positional `NOTE` required; `--format` default `Table`; `--no-color`; `--allow-empty`) to `ft/src/cmd/notes.rs`; register in `NotesCommand` enum + `run` dispatch.
- [ ] 4.2 Implement `run_related`: discover vault, build graph (`Scan::default()`), resolve note-or-ghost via `resolve_note_or_ghost`, call `score_related`, map `RelatedScore` → `RelatedRow` (resolving candidate path via `graph.node(note_id)`), dispatch to the four renderers.
- [ ] 4.3 Exit 1 on empty result unless `--allow-empty` (parity with `ft notes backlinks`/`links`).
- [ ] 4.4 No git/blame dependency — confirm `run_related` does not call `ft_core::git` or `BlameCache`.

## 5. ft: integration tests for `ft notes related`

- [ ] 5.1 Create `ft/tests/notes_related.rs` mirroring `ft/tests/notes_journal.rs` (no git needed for the scoring path; build a vault with `.obsidian/` + co-occurring paragraphs).
- [ ] 5.2 Note-target test: `ft notes related "Foo"` prints Bar (score 3) and Baz (score 1) in table + json; verify `title`, `score`, `already_in_related`, path field.
- [ ] 5.3 Ghost-target test: `ft notes related "[[Phantom]]"` succeeds and prints scored concepts; verify no row has `already_in_related == true`.
- [ ] 5.4 `--allow-empty` test: empty result exits 0 with the flag, 1 without.
- [ ] 5.5 Already-in-related test: note with a `## Related` section shows the alias row marked (`already_in_related == true` in json).
- [ ] 5.6 `cargo test --test notes_related` green.

## 6. ft: TUI modal reframe (read+write)

- [ ] 6.1 In `ft/src/tui/tabs/graph.rs::render_related_modal`, change the modal title from `" Update Related: {} "` → `" Related: {} "`.
- [ ] 6.2 Update the `HelpSection` entry wording (drop "updater"/"update"; reflect unified read/write panel) — the `("Shift+R", …)` binding stays.
- [ ] 6.3 Update `graph.related` `CommandDef` description in `ft/src/tui/tabs/graph.rs` (e.g. "Open the Related panel for the selected note").
- [ ] 6.4 Update `related.*` `CommandDef` descriptions in `ft/src/tui/modal_commands.rs` to the unified framing (no new commands, no keymap changes).
- [ ] 6.5 Confirm `build_related_modal_for_id` still toasts on ghost rows and does not open the modal (Option A: modal stays Note-only).

## 7. ft: TUI tests + snapshots

- [ ] 7.1 Run the existing `graph_related_modal_*` tests in `ft/src/tui/tests.rs` — they assert content (`[[Alias]]`/`[[C]]`/`[[D]]`) and `Shift+r` in help, not the old title, so they should pass unchanged; fix any that broke.
- [ ] 7.2 Add/adjust an assertion that the modal title reads `Related:` (not `Update Related:`).
- [ ] 7.3 Add an assertion that opening the modal and pressing Esc without toggling writes nothing (read-safe).

## 8. Build invariants

- [ ] 8.1 `cargo build --release`
- [ ] 8.2 `cargo test --workspace`
- [ ] 8.3 `cargo clippy --workspace --tests -- -D warnings`
- [ ] 8.4 `cargo fmt --check`
- [ ] 8.5 Regenerate committed reference: `cargo run --release -q -- commands docs > docs/keybindings.md`
- [ ] 8.6 `cargo run --release -q -- commands docs --check` (keybindings.md in sync with the registry)

## 9. Archive

- [ ] 9.1 Run the openspec archive flow (`.pi/skills/openspec-archive-change`) once all build invariants are green.
