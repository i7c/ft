# Tasks: remove the deprecated gather functionality

## 1. ft-core engine removal

- [x] 1.1 Delete `ft-core/src/gather.rs` (module, helpers, and tests) and remove `pub mod gather;` from `ft-core/src/lib.rs`
- [x] 1.2 Move `resolve_related_aliases` + `find_related_range` (and their tests) from gather into `ft-core/src/related.rs`; update the `use crate::gather::resolve_related_aliases;` import and any doc comments in `related.rs`
- [x] 1.3 Remove `From<&GatherEntry> for SynthSource` and its test from `ft-core/src/synth/source.rs`; update module docs (feed callers = `RecentEntry` only)
- [x] 1.4 Remove `parse_synth_targets` from `ft-core/src/synth/callout.rs` (gather-tab-only consumer) and its tests
- [x] 1.5 Remove `last_synth_watermark` from `ft-core/src/synth/accrete.rs` (gather-tab-only consumer) and its tests; keep `filter_missing` and append-dedup unchanged
- [x] 1.6 Remove `Tui::show_gather` from `ft-core/src/config.rs`; update `ft-core/src/recent.rs` and `ft-core/src/frontmatter.rs` doc comments that reference gather

## 2. CLI removal and refactor

- [x] 2.1 `ft/src/cmd/notes.rs`: remove `NotesCommand::Gather`, `GatherArgs`, `run_gather`, `gather_feed_rows`, `render_gather_table`, `render_gather_json`, `resolve_link_arg`, and the dispatch arm — keeping `resolve_note_or_ghost` (used by `related`), `resolve_window` (used by `recent`), `cited_in_of` (used by `recent`), and the shared `output::feed` usage
- [x] 2.2 `ft/src/cmd/synth.rs`: refactor `pick_paragraph` to construct `SynthSource` directly (drop the `GatherEntry` intermediate and its fabricated date/matched fields); remove the `ft_core::gather::GatherEntry` import; verify `--from` and `--sort date` still work
- [x] 2.3 Update gather references in module doc comments in `ft/src/cmd/search.rs` and `ft/src/cmd/pulse.rs`

## 3. TUI shared infra (keep-alive moves)

- [x] 3.1 Create `ft/src/tui/widgets/feed_text.rs` with the moved helpers `inline_markdown_spans`, `wrap_line`, `pad_to_width`, `citation_badge_line`, `citation_detail_line`; export them from `ft/src/tui/widgets/mod.rs`; update `feed_split.rs` module doc (Recent-only)
- [x] 3.2 `ft/src/tui/tab.rs`: remove `GatherTarget`, `GatherWindow`, `MultiTargetRequest`, `AppendOrReplaceMode`, `TabKind::Gather`, the `AppRequest::{GatherFor, GatherForMulti, GatherAddSources, GatherCommitSources}` variants and their Debug arms, and the `Tab::queue_gather_for` / `queue_gather_for_multi` / `queue_gather_add_sources` / `queue_gather_commit_sources` default hooks
- [x] 3.3 `ft/src/tui/keymap.rs`: remove the `"tab/gather"` arm from `parse_scope`
- [x] 3.4 `ft/src/tui/mod.rs`: drop `GATHER_COMMANDS` from the registry slices and `GATHER_KEYMAP` from the `validate_keymap` scope bases

## 4. TUI Gather tab removal

- [x] 4.1 Delete `ft/src/tui/tabs/gather.rs`
- [x] 4.2 `ft/src/tui/app.rs`: remove the gather import, the `gather_overlay`, the Gather tab registration in `build_tabs_with_overlays`, the `service_simple` arms for `GatherFor`/`GatherForMulti`/`GatherAddSources`/`GatherCommitSources`, the `queue_gather_for_tab_test` / `queue_gather_for_multi_test` helpers, the `show_gather = true` line in `for_test`, and `GatherTab` from the `for_test_with_clock*` tab lists
- [x] 4.3 `ft/src/tui/modal.rs` + `modal_commands.rs`: remove `GatherSourcesModal`, `GatherAppendOrReplaceModal`, the `ActiveModal::{GatherSources, GatherAppendOrReplace}` variants, every match arm over them (handle_event / render / keymap_help / scope_name / commands / keymap / dispatch_command), their `Modal` impls, and `JOURNAL_SOURCES_COMMANDS` / `JOURNAL_APPEND_REPLACE_COMMANDS`
- [x] 4.4 `ft/src/tui/widgets/picker.rs`: remove `GatherSourceHit`, `GatherSourcePickerSource`, and their tests; update `ft/src/tui/widgets/mod.rs` exports

## 5. Synth-send flow simplification

- [x] 5.1 `ft/src/tui/synth_send.rs`: drop the `new_only` parameter from `SynthSendState::PickExisting`, `NonSynthPrompt`, `SynthSendHost::synth_sources`, `open_existing`, `on_existing_picked`, and `commit_send`; drop `PickContextNote`, `open_context_note`, and the `on_context_note_picked` trait method; update module docs (Search + Recent hosts, no watermark, no context mode)
- [x] 5.2 `ft/src/tui/tabs/search.rs` and `ft/src/tui/tabs/recent.rs`: switch imports to `widgets::feed_text`, drop the `_new_only` parameter from their `synth_sources` impls

## 6. Graph and Pulse rewiring

- [x] 6.1 `ft/src/tui/tabs/graph/commands.rs`: rename `graph.journal` → `graph.search-mentions` and `graph.add-to-journal-sources` → `graph.search-mentions-multi` (chords stay `J` / `Ctrl+J`), updating descriptions
- [x] 6.2 `ft/src/tui/tabs/graph/dispatch.rs`: implement the rewired handoffs — `J` raises `AppRequest::SearchWithQuery { query: "[[<title>]]", any: false }` for the cursor Note/Ghost row (toast otherwise); `Ctrl+J` raises it with the multi-selection (or cursor) Note/Ghost rows as `[[…]]` clauses, `any: true`; update the stale ghost-promote toast wording ("seeded journal" → search-sourced scaffold)
- [x] 6.3 `ft/src/tui/tabs/graph/mod.rs`: update the `help_sections()` "Cross-tab" rows for `J` / `Ctrl+J` to describe the Search handoffs
- [x] 6.4 `ft/src/tui/tabs/pulse.rs`: rename `pulse.handoff-to-gather` → `pulse.handoff-to-search` (command def, keymap binding, dispatch arm, help section); update the module doc
- [x] 6.5 Update the gather reference in the `ft/src/tui/notes_actions/paragraph_synth.rs` module doc

## 7. Tests

- [x] 7.1 Delete `ft/tests/notes_journal.rs` and `ft/tests/journal_multi_link_cli.rs`; remove `gather_is_deprecated_hidden_and_still_works` from `ft/tests/search_cli.rs` and add an unknown-subcommand assert for `ft notes gather`; trim the journal half of `ft/tests/citation_badges.rs` (keep the `ft notes recent` badge/uncited coverage)
- [x] 7.2 `ft/src/tui/tests/synthesis.rs`: remove the gather/journal tests, helpers, and fixtures (journal_tab_*, journal_*, graph_shift_j_*, graph_ctrl_j_*, multi_target_gather_*, gather_blocks/scroll/citation vaults); keep capture/keymap/pulse/reslice tests; port the send-to-synth `s`-picker and dedup tests to `ft/src/tui/tests/search.rs`; delete the gather-related insta snapshots (`journal_entry_blocks_80x24.snap`, `ft__tui__tests__synthesis__journal_*`)
- [x] 7.3 `ft/src/tui/tests/tasks.rs`: drop the Gather step from the tab-cycle test and fix the indices/asserts
- [x] 7.4 `ft/src/tui/tests/mod.rs`: fix the "after Gather" comment and any tab-count asserts
- [x] 7.5 `ft/src/cmd/commands.rs`: drop the `gather.open-sources-manager` registry assert; add asserts for `graph.search-mentions`, `graph.search-mentions-multi`, and `pulse.handoff-to-search`
- [x] 7.6 Sweep `rg -i gather` across `ft/` and `ft-core/` sources; fix any remaining compile-time or test references (excluding the historical premise-review doc)

## 8. Docs

- [x] 8.1 Regenerate `docs/keybindings.md` via `cargo run --release -q -- commands docs > docs/keybindings.md`
- [x] 8.2 Update `docs/config.md` (remove the `show_gather` row and `tab/gather` keymap-scope comment; reword the `[tui]` section)
- [x] 8.3 Update `docs/guide/tui.md` (drop Gather from the tab list, renumber, fix the Pulse handoff wording)
- [x] 8.4 Update `docs/guide/graph.md` (remove the `ft notes gather` mention; reword the `J` key row), `docs/guide/synthesis.md` (drop the deprecation note and gather flow references), `docs/guide/index.md`, and `docs/guide/vault-and-config.md`
- [x] 8.5 Update `docs/architecture.md` (engine map, Gather tab sections, §"A new TUI tab", deprecation notes) and `README.md` (command table); fix the two "gather entry" mentions in `docs/commands.md`; leave `docs/2026-07-19-premise-review.md` as a historical artifact

## 9. Final verification

- [x] 9.1 Run all five build invariants clean: `cargo build --release`, `cargo test --workspace`, `cargo clippy --workspace --tests -- -D warnings`, `cargo fmt --check`, `cargo run --release -q -- commands docs --check`
- [x] 9.2 Final sweep: `rg -i gather` returns only the historical premise-review doc and openspec archives; `ft notes gather` exits with an unknown-subcommand error; `ft commands list` no longer shows gather/pulse.handoff-to-gather
- [x] 9.3 Delete the empty canonical spec folders (`openspec/specs/journal-tui-tab`, `openspec/specs/notes-journal`, `openspec/specs/graph-to-journal-jump`, and the leftover empty `openspec/specs/synth-grow`) at archive/sync time, and confirm the synced `graph-to-search-jump` spec exists
