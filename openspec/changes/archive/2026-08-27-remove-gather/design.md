# Design: remove the deprecated gather functionality

## Context

The synthesis flow's sourcing front-end is search, not gather: `ft notes
search` feeds `synth scaffold --search`, the Pulse tab's Enter handoff, and
ghost promotion; `ft notes recent` keeps the blame-date feed alive. Gather
survives only as deprecated surface — `ft_core::gather` (the engine),
`ft notes gather` (hidden subcommand), and the opt-in Gather TUI tab
(`[tui] show_gather`, default off) — plus a thin layer of support code the
deprecation kept alive (`parse_synth_targets`, `last_synth_watermark`, the
`new_only`/context-note branches of the shared synth-send flow).

Constraints from the workspace: plan/apply split for mutations; `write_atomic`
for all file writes; the CLI is the contract for ft.nvim (which consumes only
`quote`/`export` — unaffected); the TUI is single-threaded except two producer
threads; no backwards-compat shims; `recent`, `related`, and the citation layer
stay — they share helpers with gather and must keep working after the removal.

## Goals / Non-Goals

**Goals:**
- Delete the gather engine, the hidden CLI subcommand, and the TUI Gather tab.
- Keep the shared seams intact: `related`'s alias resolution, `recent`'s
  renderers/badges, the Search/Recent send-to-synth flows, `synth --from`.
- Rewire the graph tab's cross-tab handoffs (`J`, `Ctrl+J`) to the Search tab
  and rename the stale `pulse.handoff-to-gather` command.
- Leave the build green: five invariants (`build`, `test`, `clippy`, `fmt`,
  `commands docs --check`), regenerated `docs/keybindings.md`.

**Non-Goals:**
- Removing the citation layer (`--uncited`, badges) — still consumed by
  `ft notes recent` and the Recent tab (the archived search change deferred it,
  but recent keeps it alive, so it is out of scope here).
- Changing `recent`, `related`, search, scaffold sourcing, or the callout
  grammar.
- Adding a compat shim for `[tui] show_gather` or the removed subcommand.

## Decisions

### D1. Delete `ft_core::gather`, move its two live helpers out
`build_gather` / `GatherEntry` / `GatherReport` / `heading_chain_targets` and
their tests die with the engine. `resolve_related_aliases` +
`find_related_range` move verbatim into `ft-core/src/related.rs` — the only
remaining consumer is `score_related`, and the helpers are self-contained
(graph + headings, no blame). Alternative considered: keep `gather.rs` as a
thin home for the two helpers. Rejected: the module name would mislead once
the feed is gone; `related.rs` is their natural home and already documents the
Related-section semantics.

### D2. `SynthSource` becomes the only scaffold input; `--from` builds it directly
`ft/src/cmd/synth.rs::pick_paragraph` currently fabricates a `GatherEntry`
(date via blame, `matched: vec![]`) and immediately lowers it via
`From<&GatherEntry>`. It is refactored to construct `SynthSource` directly —
the fabricated fields are dropped on conversion anyway, so the blame lookup is
dead weight there (it only fed the never-surfaced `date`). The `--sort date`
search path still uses blame, unchanged. `From<&GatherEntry>` and the
gather entry in `synth::source.rs`'s tests are removed; `From<&RecentEntry>`
stays.

### D3. TUI render helpers move to `ft/src/tui/widgets/feed_text.rs`
`inline_markdown_spans`, `wrap_line`, `pad_to_width` (Search + Recent) and
`citation_badge_line`, `citation_detail_line` (Recent) are extracted from the
gather tab into a new `widgets/feed_text.rs` module; `widgets/mod.rs` re-exports
them. Alternative considered: folding them into `feed_split.rs` (the shared
feed geometry widget). Rejected: `feed_split.rs` owns split geometry, not row
content; the helpers are text/badge formatting used by tabs that do not mount
the split widget uniformly.

### D4. Synth-send flow loses the gather-only branches
`ft/src/tui/synth_send.rs` drops: the `new_only` watermark parameter through
`SynthSendHost::synth_sources` (Search and Recent already ignore it — the
gather host was the only honoring implementation), and the context-note flow
(`PickContextNote`, `open_context_note`, `on_context_note_picked`). The `s`/`S`
pickers and the non-synth 3-way prompt stay, unchanged. The gather tab's `n`
(new-only) and `o` (context) chords die with the tab; Search/Recent never bound
them.

### D5. Graph handoffs rewire to Search; Pulse command renamed
- `J` (`graph.journal`) → `graph.search-mentions`: raises
  `AppRequest::SearchWithQuery { query: "[[<title>]]", any: false }` for the
  cursor Note/Ghost row; toasts otherwise.
- `Ctrl+J` (`graph.add-to-journal-sources`) → `graph.search-mentions-multi`:
  lowers the multi-selection (or cursor row) to `[[…]]` clauses, `any: true`
  (the old multi-target OR semantics, exactly the pattern Pulse already uses).
- `pulse.handoff-to-gather` → `pulse.handoff-to-search`: behavior is already
  `SearchWithQuery`; only the registry name, keymap, dispatch, help text, and
  regenerated docs change.
The `AppRequest::{GatherFor, GatherForMulti, GatherAddSources,
GatherCommitSources}` variants, `Tab::queue_gather_*` hooks, `GatherTarget`,
`GatherWindow`, `MultiTargetRequest`, `AppendOrReplaceMode`, and
`TabKind::Gather` are deleted with the tab. Alternatives considered: deleting
the graph handoffs outright — rejected: `J`→Search preserves the "jump to the
note's mentions" affordance users have; deleting Pulse's command without
renaming — rejected: the stale name would lie in `ft commands list` and the
`?` overlay.

### D6. Config field removed outright
`Tui::show_gather` is deleted. `Tui` carries `serde(deny_unknown_fields)`, so
a user config with `show_gather = true` fails to load with a clear unknown-field
error; the removal is called out as BREAKING in the proposal and docs, and the
config docs example is deleted. No compat shim per the workspace convention.

### D7. `parse_synth_targets` and `last_synth_watermark` die with the tab
`synth::callout::parse_synth_targets` (used only by the gather tab's `o`
flow) and `accrete::last_synth_watermark` (used only by the gather tab's `n`
flow) are removed with their tests. `upsert_synth_frontmatter`,
`is_synth_note`, and `accrete::filter_missing` stay — the shared mark-and-append
flow and append-dedup depend on them.

### D8. Test strategy
- Delete the gather CLI suites (`ft/tests/notes_journal.rs`,
  `journal_multi_link_cli.rs`, the `gather_is_deprecated_hidden_and_still_works`
  test, the journal half of `citation_badges.rs`) and the gather TUI tests in
  `synthesis.rs` (~25 tests + helpers + 3 snapshots), the `tasks.rs` tab-cycle
  gather step, and the registry assert in `cmd/commands.rs`.
- Port two send-to-synth tests to the Search tab (`s` opens the picker;
  dedup-on-append) so the shared flow keeps TUI coverage after its only
  test-driving host disappears (decision confirmed by user).
- A new `search_cli.rs`-style assert that `ft notes gather` is an unknown
  subcommand guards the removal (cheap regression net).
- `insta` snapshots that reference gather output are deleted/regenerated;
  `ft commands docs --check` regenerates `docs/keybindings.md`.

## Risks / Trade-offs

- **Parser quirk in openspec delta specs** — requirement paragraphs wrapped so
  a line begins with an inline-code token are truncated by the openspec parser.
  → Mitigation: kept requirement paragraphs unwrapped at code tokens; validated
  all 52 deltas parse fully via `openspec change show --deltas-only`.
- **User configs with `show_gather = true` break** → Mitigation: BREAKING
  marker in proposal, config docs updated; error is clear
  (unknown-field) and the fix is deleting one line.
- **Loss of TUI coverage for the shared send-to-synth flow** → Mitigation:
  ported the two highest-value tests to the Search tab (D8).
- **Anything referencing gather still compiles after the engine removal** →
  Mitigation: exhaustive inventory in the proposal/tasks; the five build
  invariants are the safety net, and `rg -i gather` should come back clean
  except for the historical premise-review doc and the archive.
- **The `graph-to-journal-jump` spec name outlives its content** →
  Mitigation: the capability's requirements are fully removed and replaced by
  `graph-to-search-jump`; the empty canonical folder is deleted at archive
  time (captured as a task).

## Migration Plan

1. Land the change as one commit (engine + CLI + TUI + tests + docs).
2. No runtime migration: the CLI subcommand and config flag simply stop
   existing. Users with `show_gather = true` remove the line.
3. Rollback: revert the commit; the pre-change binary/config surface returns
   as-is (no data or format changes are made).

## Open Questions

None — the five decisions (D1–D5) were confirmed by the user before proposal
writing.
