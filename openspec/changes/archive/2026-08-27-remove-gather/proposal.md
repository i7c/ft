# Remove the deprecated gather functionality

## Why

The paragraph search engine has proven out as the synthesis flow's sourcing
front-end: it replaced gather in the default TUI lineup, the Pulse handoff, ghost
promotion, and `ft synth scaffold --search`, and has been in daily use without
regression. Gather — the graph-walk engine, the hidden `ft notes gather`
subcommand, and the opt-in TUI Gather tab — is now dead weight held alive only by
deprecation flags: a ~1,100-line engine plus a ~2,000-line TUI tab plus a hidden
CLI surface, all shadowed by search. The archived `add-paragraph-search` change
explicitly deferred this removal "once search has proven out"; that condition is
met. Removing gather deletes the deprecated surface and the now-unused engine,
leaving the synth flow at its minimal shape: pulse → search →
scaffold/verify/repair/reslice.

## What Changes

- **Remove `ft_core::gather`** — the whole module (`build_gather`,
  `GatherReport`, `GatherEntry`, `heading_chain_targets`, `find_related_range`,
  and its ~15 tests) and `pub mod gather;` in `lib.rs`.
- **Remove `ft notes gather`** (hidden since the search change): the
  `NotesCommand::Gather` variant, `GatherArgs`, `run_gather`, the
  `gather_feed_rows` / `render_gather_table` / `render_gather_json` renderers,
  and the gather-only `resolve_link_arg` helper. `ft notes journal` (same
  subcommand) disappears with it.
- **Remove the TUI Gather tab** — `ft/src/tui/tabs/gather.rs`, the Sources
  Manager + Append-or-Replace modals, the `GatherSourcePickerSource` picker, the
  `GATHER_COMMANDS`/`GATHER_KEYMAP` statics, tab registration and overlays,
  `TabKind::Gather`, the `AppRequest::{GatherFor, GatherForMulti,
  GatherAddSources, GatherCommitSources}` variants and `Tab::queue_gather_*`
  hooks, and the test-only queue helpers.
- **Remove gather-only support code**: `Tui::show_gather` config field,
  `callout::parse_synth_targets`, `accrete::last_synth_watermark`, and the
  `new_only` watermark + context-note branches of the shared
  `ft/src/tui/synth_send.rs` flow (Search/Recent never used them).
- **Refactor `ft synth scaffold --from`** — `pick_paragraph` currently builds a
  `GatherEntry` as an intermediate and immediately lowers it; it constructs
  `SynthSource` directly (the fabricated `date`/`matched` fields were dropped on
  conversion anyway).
- **Move shared code that outlives gather**: `resolve_related_aliases` +
  `find_related_range` move into `ft-core/src/related.rs` (the `ft notes
  related` consumer); the TUI render helpers `inline_markdown_spans`,
  `wrap_line`, `pad_to_width`, `citation_badge_line`, `citation_detail_line`
  move to a new `ft/src/tui/widgets/feed_text.rs` (Search + Recent consumers).
- **Rewire the graph tab handoffs to Search** — `J` (`graph.journal`) opens the
  Search tab prefilled with `[[note]]`; `Ctrl+J`
  (`graph.add-to-journal-sources`) opens it with the selected links as
  `[[…]]` clauses in any-mode (the old multi-target OR semantics). Commands
  renamed to `graph.search-mentions` / `graph.search-mentions-multi`.
- **Rename `pulse.handoff-to-gather` → `pulse.handoff-to-search`** — the Pulse
  tab already routes to Search; the stale command name goes.
- **Tests**: delete the gather CLI suites (`notes_journal.rs`,
  `journal_multi_link_cli.rs`, the gather deprecation test, the journal half of
  `citation_badges.rs`) and the gather TUI tests; port minimal send-to-synth
  coverage (open picker on `s`, dedup-on-append) to the Search tab tests;
  update the tab-cycle and command-registry tests.
- **Docs**: strip gather from README, `docs/guide/*`, `docs/config.md`,
  `docs/architecture.md`, `docs/commands.md`; regenerate
  `docs/keybindings.md`.
- **BREAKING**: `ft notes gather` / `ft notes journal` no longer exist
  (unknown subcommand error), and `[tui] show_gather = true` in user config now
  fails to load (`serde(deny_unknown_fields)` on `Tui`). Both were deprecated
  surfaces; the config flag removal is called out so users can delete the line.

## Capabilities

### New Capabilities
- `graph-to-search-jump`: graph tab `J` / `Ctrl+J` hand off to the Search tab
  with the selected note(s) lowered to `[[…]]` search clauses (single /
  multi any-mode), replacing the old jump to the removed Gather tab.

### Modified Capabilities
- `synthesis-review-tui-tab`: the Pulse tab's Enter handoff is renamed from
  `pulse.handoff-to-gather` to `pulse.handoff-to-search` (behavior already
  routed to Search; the requirement text and command name change).
- `synth-source-input`: the `From<&GatherEntry> for SynthSource` conversion is
  removed; only `From<&RecentEntry>` remains as the feed-side lowering seam.
- `synth-notes`: the scaffold planner's feed-lowering note drops `GatherEntry`
  (only `RecentEntry` lowers via `From`); `--from` picks build `SynthSource`
  directly.
- `notes-history`: requirements that asserted journal parity or referenced
  gather's window/badge grammar are reworded to be self-contained (behavior
  unchanged).
- `paragraph-search`: prose references to the deprecated gather (multi-target
  OR parity, "extracted from the Gather tab") are reworded.
- `paragraph-synth-tui`: the commit-path reference to "the gather tab's
  send-to-synth" is re-pointed at the shared synth-send flow.

### Removed Capabilities
- `journal-tui-tab`: all requirements removed (Gather tab deleted).
- `notes-journal`: all requirements removed (`ft notes gather` / `journal`
  deleted).
- `graph-to-journal-jump`: all requirements removed (replaced by
  `graph-to-search-jump`).

## Impact

- **`ft-core`**: delete `gather.rs`; move `resolve_related_aliases` +
  `find_related_range` into `related.rs`; remove `parse_synth_targets` /
  `last_synth_watermark`; drop `From<&GatherEntry>`; `Tui::show_gather`
  removed; doc-comment updates in `recent.rs` / `frontmatter.rs`.
- **`ft` binary**: CLI churn in `notes.rs` / `synth.rs`; TUI churn across
  `tabs/gather.rs` (deleted), `tab.rs`, `app.rs`, `modal.rs`, `modal_commands`,
  `widgets/picker.rs`, `widgets/mod.rs`, `widgets/feed_text.rs` (new),
  `synth_send.rs`, `tabs/{graph,pulse,search,recent}`, `mod.rs`, `keymap.rs`;
  `cmd/commands.rs` registry test.
- **`ft.nvim`**: unaffected — it consumes only `quote`/`export`; the CLI
  contract change is the deletion of an already-hidden subcommand.
- **Dependencies**: none new, none removed.
- **Tests**: deletion + porting as listed above; `insta` snapshots for the
  gather tab removed; `docs/keybindings.md` regenerated (build-invariant check
  `ft commands docs --check` stays green).
- **Docs**: README command table, `docs/guide/{tui,graph,synthesis,index,
  vault-and-config}.md`, `docs/config.md`, `docs/architecture.md`,
  `docs/commands.md`, `docs/keybindings.md` (regenerated).
