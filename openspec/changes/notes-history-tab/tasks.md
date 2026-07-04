## 1. Core builder (`ft_core::history`)

- [x] 1.1 Add `ft-core/src/history.rs` with `HistoryEntry` (`date`, `source_title`, `source_path`, `line_start`, `line_end`, `section_text`) and register the module in `ft-core/src/lib.rs`.
- [x] 1.2 Implement `build_history(graph, vault, window, cfg, opts, cache) -> Result<HistoryReport>`: resolve the window, compute `link_review::compute_link_review` added-lines, and iterate `Graph::nodes()` paragraph nodes.
- [x] 1.3 Include a paragraph iff `line_start..=line_end` overlaps the file's added-line set; blame only files present in the added-lines map (perf prefilter); compute each entry's date via `blame_cache::paragraph_date`.
- [x] 1.4 Exclude paragraphs whose source note has `ft-synth: true` unless `opts.include_synth`; keep periodic notes.
- [x] 1.5 Sort entries `(date desc, source_title asc, line_start asc)`, matching the journal; surface `skipped_blame` diagnostics like `JournalReport`.
- [x] 1.6 Unit tests: window inclusion/exclusion, ordering + tiebreaks, synth exclusion + `--include-synth`, blame-only-touched-files. Reuse the journal tests' git-fixture helpers.

## 2. CLI `ft notes history`

- [ ] 2.1 Extract the journal table + JSON renderers into a shared helper (in `ft/src/output/`) that both journal and history call; keep journal output byte-identical (existing `insta` snapshots are the guard).
- [ ] 2.2 Add `HistoryArgs` and a `History` variant to `NotesCommand` in `ft/src/cmd/notes.rs`: `--since`/`--range` (mutually exclusive, default `7d`), `--include-synth`, `--json`, `--no-color`; dispatch in `run`.
- [ ] 2.3 Implement `run_history`: discover vault, require a git repo (clear error otherwise), build graph, load `BlameCache`, resolve window (default `7d`), call `build_history`, render, and warn on `skipped_blame`.
- [ ] 2.4 Integration tests under `ft/tests/` (assert_cmd + git fixture): default run, `--since`/`--range`, mutual exclusion error, `--json` shape, `--include-synth`, non-git error, `NO_COLOR`.

## 3. Seeded section-move entry point

- [ ] 3.1 Add `section_move::begin_for_source(ctx, source_rel) -> SectionMoveState` reusing `advance_to_multiselect` so the modal opens at heading multi-select for a known note (no source picker).
- [ ] 3.2 Unit/snapshot test that `begin_for_source` yields a `HeadingMultiSelect` state scoped to the given note's headings.

## 4. TUI History tab

- [ ] 4.1 Add `ft/src/tui/tabs/history.rs` implementing `Tab` (with `kind() -> TabKind::History`), holding a session `BlameCache`, current window (default `7d`), and the derived feed; read data from `ctx.snapshot` only.
- [ ] 4.2 Declare `HISTORY_COMMANDS` + `HISTORY_KEYMAP`; implement `help_sections()` and a `dispatch_command` arm; re-derive feed on `on_graph_ready`/`on_focus`; add empty-state and reload.
- [ ] 4.3 Register the tab in `build_tabs_with_overlays` wrapped with `.with_keymap_overlay(...)`; add the `TabKind` variant and routing.
- [ ] 4.4 Row selection (one/many/all) → hand selected paragraphs to the synth scaffold flow targeting a chosen note; ensure no `ft-synth-target` frontmatter is written and no synth-grow step is offered.
- [ ] 4.5 Move action on the focused row → open `ActiveModal::SectionMove` via `begin_for_source(ctx, row.source_path)`; raise `ctx.request_graph_refresh()` after a completed move.
- [ ] 4.6 `TestBackend` snapshot tests under `ft/src/tui/tests/`: feed render, empty-window state, select→synth scaffold, seeded move opens at heading select.

## 5. Registry, docs, and build invariants

- [ ] 5.1 Regenerate `docs/keybindings.md` (`cargo run --release -q -- commands docs > docs/keybindings.md`) and confirm `commands docs --check` passes.
- [ ] 5.2 Update `docs/commands.md` / `docs/architecture.md` (Synthesis ritual + tabs sections) and `README.md`/CLI help to mention `ft notes history` and the History tab.
- [ ] 5.3 Run the five build invariants clean: `cargo build --release`, `cargo test --workspace`, `cargo clippy --workspace --tests -- -D warnings`, `cargo fmt --check`, `commands docs --check`.
