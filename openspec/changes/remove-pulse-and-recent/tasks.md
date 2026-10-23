# Tasks: remove the Pulse and Recent functionality

## 1. ft-core engines

- [x] 1.1 Delete `ft-core/src/pulse.rs` and remove `pub mod pulse;` from `ft-core/src/lib.rs`
- [x] 1.2 Delete `ft-core/src/recent.rs` and remove `pub mod recent;` from `ft-core/src/lib.rs`
- [x] 1.3 Delete `ft-core/src/synth/citations.rs` and its `pub mod citations;` from `ft-core/src/synth/mod.rs`
- [x] 1.4 `ft-core/src/synth/source.rs`: remove `From<&RecentEntry> for SynthSource` and its test; update the module doc (no feed lowering remains)
- [x] 1.5 `ft-core/src/git.rs`: remove the pulse-only helpers `rev_parse`, `commit_before`, `diff_changed_paths`, `diff_added_lines`, `parse_added_lines_from_diff` (+ their tests) and the "Diff helpers (used by pulse)" section header; keep blame/log helpers
- [x] 1.6 Doc-comment sweep in `ft-core`: `config.rs` (`[synth]` doc says it governs `ft notes pulse` / `exclude_prefixes` filters the link-review), `search/index.rs` (two "pulse" comment references), `synth/callout.rs`, `frontmatter.rs` — reword to search terms; keep `[synth] exclude_prefixes` and `path_excluded`
- [x] 1.7 Delete the pulse case in `ft-core/tests/real_vault.rs` (lines ~100–130) and any now-unused imports

## 2. CLI removal

- [x] 2.1 Delete `ft/src/cmd/pulse.rs` and `pub mod pulse;` in `ft/src/cmd/mod.rs`
- [x] 2.2 `ft/src/cmd/notes.rs`: remove `NotesCommand::{Recent, Pulse}`, `RecentArgs`, `run_recent`, `resolve_window`, `cited_in_of`, and their dispatch arms; drop now-unused imports (`BlameCache` if unused, `ft_core::pulse`, `ft_core::recent`, `crate::output::feed`)
- [x] 2.3 Delete `ft/src/output/feed.rs` and `pub mod feed;` in `ft/src/output/mod.rs` (no remaining consumer)
- [x] 2.4 Confirm `ft notes search`, `ft notes synth *`, `ft notes ghosts`, `ft notes related` still compile and behave unchanged

## 3. TUI removal

- [x] 3.1 Delete `ft/src/tui/tabs/pulse.rs` and `ft/src/tui/tabs/recent.rs`; drop `pub mod pulse; pub mod recent;` from `ft/src/tui/tabs/mod.rs`
- [x] 3.2 `ft/src/tui/mod.rs`: drop `PULSE_COMMANDS`/`RECENT_COMMANDS` from the registry slices and `PULSE_KEYMAP`/`RECENT_KEYMAP` from both `validate_keymap` scope lists; remove the `tab/pulse` and `tab/recent` entries
- [x] 3.3 `ft/src/tui/keymap.rs`: remove the `tab/pulse` and `tab/recent` arms from `parse_scope`
- [x] 3.4 `ft/src/tui/tab.rs`: remove `TabKind::{Pulse,Recent}` and any `Pulse`/`Recent` doc references; keep `AppRequest::SearchWithQuery` and the `install_query` hook (Graph tab uses them)
- [x] 3.5 `ft/src/tui/app.rs`: remove the `PulseTab`/`RecentTab` imports, `pulse_overlay`/`recent_overlay`, their `build_overlay("tab/…")` calls, their entries in the default tab list (renumber comment: Graph, Notes → Search → opt-in Tasks/Timeblocks), the `tab/pulse`/`tab/recent` scope mirror lists, the `for_test*` tab lists, and any test helpers that index by the old tab positions
- [x] 3.6 `ft/src/tui/snapshot.rs`: remove the `citations: CitationIndex` field and its build in the snapshot constructor; drop the `ft_core::synth::citations` import; update the `GraphSnapshot` doc comment
- [x] 3.7 `ft/src/tui/tabs/graph/mod.rs`: drop the `citations:` initializer in the `set_graph_for_test` snapshot literal
- [x] 3.8 `ft/src/tui/widgets/feed_text.rs`: remove `citation_badge_line`, `citation_detail_line`, `pad_to_width` (+ their tests) and the `CitationState` import; update `ft/src/tui/widgets/mod.rs` exports — keep `inline_markdown_spans`, `wrap_line` and the module doc (Search-owned now)
- [x] 3.9 `ft/src/tui/widgets/feed_split.rs`: update the module doc (Search-only, no Recent)
- [x] 3.10 `ft/src/cmd/commands.rs`: drop the `pulse.handoff-to-search` registry assert (and any `recent.*` asserts); keep the graph/search asserts

## 4. Test cleanup

- [x] 4.1 Delete `ft/src/tui/tests/history.rs` and its `mod history;` registration in `ft/src/tui/tests/mod.rs`
- [x] 4.2 `ft/src/tui/tests/synthesis.rs`: remove the Pulse tests and helpers (`pulse_tab_idx`, `pulse_test_vault`, `pulse_overflow_vault`, the pulse tab render/scroll/handoff tests) and the Pulse reference in the module doc; keep capture-preset and graph→Search coverage
- [x] 4.3 `ft/src/tui/tests/search.rs`: remove `pulse_handoff_opens_search_prefilled_in_any_mode` and the Pulse mention in the module doc; confirm the remaining Search tests pass
- [x] 4.4 `ft/src/tui/tests/tasks.rs`: fix the tab-cycle expectations (Search is now `3`, Tasks `4`, Timeblocks `5`) and the tab-list loop at ~line 621
- [x] 4.5 `ft/src/tui/tests/mod.rs`: update the five-tab layout assert (`1 Graph  2 Notes  3 Search`) and rename/restructure the now-three-tab snapshot test
- [x] 4.6 Delete `ft/tests/review_cli.rs`, `ft/tests/notes_history.rs`, `ft/tests/citation_badges.rs`
- [x] 4.7 Delete the now-obsolete `insta` snapshots (`default_layout_five_tabs_80x24.snap`, `history_split_layout_80x24.snap`, `notes_open_picker_recents_80x24.snap` only if it asserts tab names, and any other `.snap` mentioning Pulse/Recent), then re-accept the remaining affected frames after eyeballing the diff (tab bar + digit changes only)
- [x] 4.8 `ft/tests/real_vault_cli.rs`: delete the `notes pulse` cases; `ft/tests/search_cli.rs`, `synth_cli.rs`, `notes_*.rs` — sweep for stale references

## 5. Docs

- [x] 5.1 Regenerate `docs/keybindings.md` (`cargo run --release -q -- commands docs > docs/keybindings.md`) and confirm `ft commands docs --check` passes
- [x] 5.2 `docs/config.md`: remove the `tab/pulse` / `tab/recent` keymap-scope mentions; keep `[synth] exclude_prefixes` documented as a search filter
- [x] 5.3 `docs/architecture.md`: engine map (drop pulse/recent/citations), tab roster and tab-order section, §"Synthesis" flow (`search → scaffold/verify/repair/reslice`), any shared-snapshot field list mentioning `citations`
- [x] 5.4 Guides: `docs/guide/synthesis.md` (drop the pulse/recent sweep step), `docs/guide/notes.md` (drop the `pulse`/`recent` command entries), `docs/guide/tui.md` (tab list + digits), `docs/guide/index.md`, `docs/guide/philosophy.md` (sweep trigger = date-sorted search)
- [x] 5.5 `README.md`: command table and any Pulse/Recent tab mention in the TUI section
- [x] 5.6 `CHANGELOG.md`: add a `### Removed` entry under `## Unreleased` naming `ft notes pulse`, `ft notes recent`, the two tabs, the renumbered Search tab, the `tab/pulse`/`tab/recent` keymap-scope removal, and the `search --sort date` migration
- [x] 5.7 Leave the historical reviews (`docs/2026-*-review.md`, `docs/2026-07-19-premise-review.md`) untouched

## 6. Openspec hygiene

- [x] 6.1 Delete `openspec/changes/feed-split-layout/` (moot: Pulse/Recent/Gather all gone)
- [x] 6.2 Note in the archive step that `link-review`, `synthesis-review-tui-tab`, `notes-history`, `history-tui-tab`, and `citation-index` spec folders are removed on sync
- [x] 6.3 Confirm no other active change depends on the removed surface (`hierarchical-ft-frontmatter` mentions the files but its spec deltas touch no removed capability)

## 7. Final verification

- [x] 7.1 All five build invariants clean: `cargo build --release`, `cargo test --workspace`, `cargo clippy --workspace --tests -- -D warnings`, `cargo fmt --check`, `cargo run --release -q -- commands docs --check`
- [x] 7.2 Final sweep: `rg -i "pulse|citation|notes recent"` over `ft/ ft-core/ docs/ README.md` returns only historical review docs; `ft notes pulse` and `ft notes recent` exit 2 with clap's unknown-subcommand error; `ft commands list` shows no `pulse.*`/`recent.*`
- [x] 7.3 Manual smoke: `ft tui` shows `1 Graph  2 Notes  3 Search`, Search query + `s`/`S` send-to-synth work, `Space`/`Enter`/`o`/`a` unaffected
- [x] 7.4 `ft.nvim` follow-up commit: drop the `Pulse | ft notes pulse --json` row from `ARCHITECTURE.md`'s CLI-surface table (docs-only; no Lua depends on it)
