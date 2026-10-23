## Why

Paragraph search has become the single entry point to synthesis
(`ft notes search` CLI + the Search tab's live query, sort, and
send-to-synth flow). The two git-window feeds that preceded it — **Pulse**
(frequency-ranked links in a commit window) and **Recent** (whole-vault
paragraphs edited in a window) — are now redundant: their one remaining job
("what has been on my mind lately?") is something the search DSL answers
directly with `--sort date` and a date predicate, without a second engine,
a second set of keybindings, or a git-log scan on tab focus.

Keeping them costs a ~760-line git-diff/pulse engine, a ~550-line history
feed, a ~450-line Pulse tab and a ~700-line Recent tab, the citation-index
module that only the Recent feed consumed, and two of the seven default
tabs. This change removes both features outright — no deprecation flags, no
hidden subcommands — so the sweep flow is exactly: **search → synth**.

## What Changes

- **Remove `ft_core::pulse`** — the whole module (`compute_pulse`, `Pulse`,
  `PulseRow`, `WindowRange`, the added-lines map and its ~20 tests) and
  `pub mod pulse;` in `lib.rs`. It had exactly two consumers: the Pulse CLI
  and `recent::build_recent`.
- **Remove `ft notes pulse`** — the `NotesCommand::Pulse` variant and
  `ft/src/cmd/pulse.rs` (table/JSON renderers, `resolve_window`).
- **Remove the TUI Pulse tab** — `ft/src/tui/tabs/pulse.rs`,
  `PULSE_COMMANDS` / `PULSE_KEYMAP`, the `tab/pulse` keymap scope, tab
  registration and overlay, `TabKind::Pulse`, and the `pulse.*` commands
  (`Enter` handoff to Search included — Search keeps its own input).
- **Remove `ft_core::recent`** — the whole module (`build_recent`,
  `RecentEntry`, `RecentOptions`) and `pub mod recent;`. Its window
  selection, blame dates, and synth-note exclusion were only ever consumed
  by the Recent CLI and tab.
- **Remove `ft notes recent`** — the `NotesCommand::Recent` variant,
  `RecentArgs`, `run_recent`, `cited_in_of`, the `--uncited-only` /
  `--include-synth` flags, and the shared feed output module
  `ft/src/output/feed.rs` (whose only consumer it was).
- **Remove the TUI Recent tab** — `ft/src/tui/tabs/recent.rs`,
  `RECENT_COMMANDS` / `RECENT_KEYMAP`, the `tab/recent` keymap scope, tab
  registration and overlay, `TabKind::Recent`, and the `recent.*` commands.
- **Remove `ft_core::synth::citations` (`CitationIndex`)** — it existed to
  put `cited:` / `cited*:` badges on history rows (and `cited_in` in
  `ft notes recent --json`); with the feed gone it has no consumer. Drops
  `CitationIndex::build`/`lookup`, the `GraphSnapshot::citations` field and
  its per-rebuild build cost, and `widgets::feed_text::{citation_badge_line,
  citation_detail_line}`.
- **Remove the now-dead git diff helpers** — `git::{rev_parse,
  commit_before, diff_changed_paths, diff_added_lines,
  parse_added_lines_from_diff}` were pulse-only.
- **Remove `From<&RecentEntry> for SynthSource`** — the last feed-shaped
  lowering seam; `SynthSource` is now constructed directly by every caller.
- **Tabs renumber** — the default lineup becomes `1 Graph  2 Notes
  3 Search`; Search moves from `5` to `3`. No other tab changes.
- **Delete the moot `openspec/changes/feed-split-layout`** — it proposed
  layout fixes for Pulse, Recent, and Gather, all of which no longer exist.
- **Keep `ft-core/src/recents.rs`** (the per-vault *recently-opened* log
  behind pickers). It is a different feature with no dependency on either
  feed; the empty-input picker keeps surfacing recent opens.
- **Keep** `[synth] exclude_prefixes` (used by search), `blame_cache`
  (search `--sort date`), `widgets::{feed_split,feed_text}` and the
  `synth_send` flow (both Search-owned now), and `AppRequest::SearchWithQuery`
  (used by the Graph tab's `J` / `Ctrl+J` handoffs).
- **BREAKING**: `ft notes pulse` and `ft notes recent` no longer exist
  (clap unknown-subcommand error, exit 2), the Pulse and Recent tabs are
  gone from the TUI, and any `[keymap]` override under `tab/pulse` or
  `tab/recent` now fails validation (`ft commands check-keymap` errors,
  `ft commands list` no longer shows the commands). Scripts and keybinding
  overrides must be dropped.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `note-flow-naming`: drops `recent` / `pulse` from the subcommand and tab
  naming lists, the hard-coded tab order, and the tab-strip example; the
  naming contract now covers the surviving verbs and tabs only.
- `paragraph-search`: removes the "Pulse handoff to Search" requirement
  (the Search tab's own input replaces it) and the "shared with the Recent
  tab" clause on send-to-synth.
- `synth-source-input`: removes the `From<&RecentEntry>` conversion
  requirement; `SynthSource` remains the honest 4-field input type, now
  with no feed lowering.
- `synth-notes`: the scaffold planner note no longer names `RecentEntry` as
  a feed caller; all callers construct `SynthSource` directly.
- `paragraph-synth-tui`: reworded to reference the Search tab as the
  feed-driven send-to-synth sibling (Recent is gone).
- `vault-scan`: removes the history-feed / link-review cases from the
  scan-served-consumer requirements; the picker, mtime, and Related-alias
  cases are unchanged.
- `ghost-promotion`: drops the "same dedup rule as `ft notes pulse`"
  cross-reference (ghost ranking itself is graph-only and unchanged).
- `note-flow-story`: the story no longer names `ft notes pulse` /
  `ft notes recent` as the note-flow sweep surfaces.

### Removed Capabilities

- `link-review`: all requirements removed (`ft core pulse` engine and
  `ft notes pulse` deleted).
- `synthesis-review-tui-tab`: all requirements removed (Pulse tab deleted).
- `notes-history`: all requirements removed (`ft_core::recent` engine and
  `ft notes recent` deleted).
- `history-tui-tab`: all requirements removed (Recent tab deleted).
- `citation-index`: all requirements removed (`CitationIndex` had no
  consumer left once history badges went).

## Impact

- **`ft-core`**: delete `src/pulse.rs`, `src/recent.rs`,
  `src/synth/citations.rs`; `lib.rs` and `synth/mod.rs` module lists;
  `git.rs` diff helpers + tests; `synth/source.rs` `From` impl + test;
  doc-comment updates in `config.rs`, `search/index.rs`,
  `synth/callout.rs`, `frontmatter.rs`.
- **`ft` binary**: delete `cmd/pulse.rs`, `output/feed.rs`,
  `tui/tabs/{pulse,recent}.rs`, `tui/tests/history.rs`; churn in
  `cmd/{mod,notes}.rs`, `output/mod.rs`, `tui/{app,mod,keymap,tab,snapshot}.rs`,
  `tui/tabs/mod.rs`, `tui/widgets/{mod,feed_text}.rs`, `cmd/commands.rs`.
- **Tests**: delete `ft/tests/{review_cli,notes_history,citation_badges}.rs`
  and the `ft-core/tests/real_vault.rs` pulse case; port/repair the tab-cycle,
  registry, and snapshot tests; drop the Pulse/Recent `insta` snapshots.
- **Docs**: README command table, `docs/guide/{index,notes,synthesis,tui,
  philosophy}.md`, `docs/config.md` (keymap scopes), `docs/architecture.md`
  (engine map, tab roster, synthesis section); regenerate
  `docs/keybindings.md`; CHANGELOG entry.
- **`ft.nvim`**: docs-only follow-up — `ARCHITECTURE.md` lists
  `ft notes pulse --json` as an existing CLI surface; no Lua code calls
  `pulse` or `recent`.
- **Dependencies**: none added; none removed.
