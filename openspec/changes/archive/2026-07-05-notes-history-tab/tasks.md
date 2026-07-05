## 1. Core builder (`ft_core::history`)

- [x] 1.1 Add `ft-core/src/history.rs` with `HistoryEntry` (`date`, `source_title`, `source_path`, `line_start`, `line_end`, `section_text`) and register the module in `ft-core/src/lib.rs`.
- [x] 1.2 Implement `build_history(graph, vault, window, cfg, opts, cache) -> Result<HistoryReport>`: resolve the window, compute `link_review::compute_link_review` added-lines, and iterate `Graph::nodes()` paragraph nodes.
- [x] 1.3 Include a paragraph iff `line_start..=line_end` overlaps the file's added-line set; blame only files present in the added-lines map (perf prefilter); compute each entry's date via `blame_cache::paragraph_date`.
- [x] 1.4 Exclude paragraphs whose source note has `ft-synth: true` unless `opts.include_synth`; keep periodic notes.
- [x] 1.5 Sort entries `(date desc, source_title asc, line_start asc)`, matching the journal; surface `skipped_blame` diagnostics like `JournalReport`.
- [x] 1.6 Unit tests: window inclusion/exclusion, ordering + tiebreaks, synth exclusion + `--include-synth`, blame-only-touched-files. Reuse the journal tests' git-fixture helpers.

## 2. CLI `ft notes history`

- [x] 2.1 Extract the journal table + JSON renderers into a shared `ft/src/output/feed.rs` (`FeedRow` + `render_table`/`render_json`) that both journal and history call; journal output byte-identical (`matched` via `skip_serializing_if`), guarded by existing journal integration tests.
- [x] 2.2 Add `HistoryArgs` and a `History` variant to `NotesCommand` in `ft/src/cmd/notes.rs`: `--since`/`--range` (mutually exclusive, default `7d`), `--include-synth`, `--json`, `--no-color`; dispatch in `run`.
- [x] 2.3 Implement `run_history`: discover vault, require a git repo (clear error otherwise), build graph, load `BlameCache`, resolve window (default `7d`), call `build_history`, render, and warn on `skipped_blame`.
- [x] 2.4 Integration tests in `ft/tests/notes_history.rs` (assert_cmd + git fixture): default window, `--range` `--json` shape (no `matched`), mutual-exclusion error, `--include-synth`, non-git error, `NO_COLOR`.

## 3. Seeded section-move entry point

- [x] 3.1 Add `section_move::begin_for_source(ctx, source_rel) -> Option<SectionMoveState>` (the shared primitive; `advance_to_multiselect` now delegates to it) so the modal opens at heading multi-select for a known note (no source picker).
- [x] 3.2 Unit tests: `begin_for_source` yields a `HeadingMultiSelect` scoped to the note's headings, and `None` when the note has no headings.

## 4. TUI History tab

- [x] 4.1 Add `ft/src/tui/tabs/history.rs` implementing `Tab` (with `kind() -> TabKind::History`), holding a session `BlameCache`, current window (default `7d`), and the derived feed; read data from `ctx.snapshot` only.
- [x] 4.2 Declare `HISTORY_COMMANDS` + `HISTORY_KEYMAP`; implement a `dispatch_command` arm; re-derive feed on `on_graph_ready`/`on_focus` (generation-tracked catch-up); empty-state + reload. (Help overlay is registry-generated; no `help_sections()` override needed.)
- [x] 4.3 Register the tab in `build_tabs_with_overlays` + `for_test` wrapped with `.with_keymap_overlay(...)`; add the `TabKind::History` variant and the `registry::build()` command slice.
- [x] 4.4 Row selection (one/many/all) → synth scaffold flow (reuses journal's `SynthSendState` overlay + core `plan/apply_synth_scaffold`); no `ft-synth-target` frontmatter, no synth-grow/new-only step.
- [x] 4.5 Move action → open `ActiveModal::SectionMove` via `begin_for_source(ctx, row.source_path)`; refresh raised inside the shared `commit_move` so any completed move refreshes the snapshot.
- [x] 4.6 `TestBackend` tests in `ft/src/tui/tests/history.rs`: feed render (windowed), empty-window state, seeded section-move modal opens, send-to-synth opens the existing-note picker.

## 5. Registry, docs, and build invariants

- [x] 5.1 Regenerated `docs/keybindings.md` via `ft commands docs` (added the `tab/history` section); `commands docs --check` passes.
- [x] 5.2 Updated `docs/architecture.md` (Synthesis ritual `build_history` + seven-tabs section), `docs/guide/notes.md` (new "The History feed" section), `docs/guide/tui.md` (seven-tabs table, incl. the previously-missing Review row), and `README.md`. CLI help is auto-generated from the `History` clap doc comments. (`docs/commands.md` needs no edit — it documents the command system generally; per-tab commands live in the generated keybindings.md.)
- [x] 5.3 Five build invariants clean: `cargo build --release`, `cargo test --workspace` (35 binaries ok), `cargo clippy --workspace --tests -- -D warnings`, `cargo fmt --check`, `commands docs --check`.
