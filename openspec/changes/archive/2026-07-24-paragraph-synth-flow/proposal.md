## Why

The synth flow today only accepts *pre-computed* paragraphs — paragraphs
the gather feed (link-targeted) or the recent feed (time-windowed)
decided to surface. There is no way to say "open note A, pick *these*
specific paragraphs, pin them into a discussion note." For the
collection-note workflow (a note accumulates topics over time; later,
some are lifted into a synthesis), the user must wait for paragraphs to
appear in a feed, or edit the synth note by hand. We need a
source-driven copy-to-synth flow: pick a note, pick its paragraphs,
send them as protected `[!ft-source]` callouts to a target.

## What Changes

- **New TUI flow "paragraph-synth"** triggered by `y` ("yank → pin"):
  - On the **Graph tab** with a Note node focused: seeds directly into
    the paragraph-multi-select step (source = selected note).
  - On the **Notes tab**: opens a source-note fuzzy picker first, then
    proceeds to paragraph-multi-select.
  - Entry guard: if the git working tree is not clean, the flow aborts
    with a toast (`synth needs a clean working tree …`) and never opens
    the modal. Protected sections pin to HEAD, so a dirty tree would
    produce unverifiable pins.
- **New paragraph-multi-select step** with a split-pane layout reusing
  `render_feed_split` (list on top, preview below). `Space` toggles
  paragraphs; `Enter` proceeds to target pick. Selections default to
  the full paragraph line range.
- **Range-adjust (shrink-only)** for the focused pick: `[` trims the
  top line, `]` trims the bottom line, `r` resets to the full
  paragraph. The effective range is the pin's line range; its body is
  re-sliced from the source content. Floor: always ≥ 1 line remains.
  Grow beyond paragraph bounds is explicitly out of scope (multi-select
  spans adjacent paragraphs instead).
- **Target pick + commit** reuses the gather tab's existing
  send-to-synth split: pick an existing note (`s`) or create a new one
  (`S`, folder → title → template + vars). Commit runs the existing
  `plan_synth_scaffold` + `apply_synth_scaffold` and hands off to
  `$EDITOR`. No compose/reorder step; sections append in selection
  order.
- **Unified input type** `SynthSource` (new, in `ft-core::synth`): the
  honest 4-field shape the scaffold actually consumes (`source_path`,
  `line_start`, `line_end`, `body`). `plan_synth_scaffold` and
  `accrete::filter_missing` switch from `&[GatherEntry]` /
  `Vec<GatherEntry>` to `&[SynthSource]` / `Vec<SynthSource>`.
  `GatherEntry` and `RecentEntry` gain `impl From<&Self> for
  SynthSource` and lower at the call boundary. This removes the
  dishonest `date` / `matched` fields the feed callers had to
  fabricate, and gives the new flow a native input.
- **New TUI modal variant** `ActiveModal::ParagraphSynth` driven
  through the modal driver (modal-first dispatch), following the
  section-move modal's step-state-machine shape but committing via the
  gather flow's plan/apply path.
- **New commands + keymap rows** for both Graph and Notes tabs, plus
  the modal's own command slice; `docs/keybindings.md` regenerated.

## Capabilities

### New Capabilities
- `paragraph-synth-tui`: the `y`-triggered copy-to-synth flow —
  entry guard, paragraph multi-select with shrink range-adjust, target
  pick, scaffold commit. Covers the Graph-tab (tree-seeded) and
  Notes-tab (picker-seeded) entry points and the modal's step machine.
- `synth-source-input`: the `SynthSource` input type and the
  `From<&GatherEntry>` / `From<&RecentEntry>` conversions; the
  scaffold + accrete input-type migration. The honest boundary between
  paragraph sources and the pinning engine.

### Modified Capabilities
- `tui-modal-driver`: gains a new `ActiveModal::ParagraphSynth`
  variant following the existing modal-first dispatch contract.
- `tui-commands`: gains the `graph.synth-from-note` and
  `notes.synth-from-note` commands and their keymap rows.
- `synth-notes`: the scaffold planner's input contract changes from
  `&[GatherEntry]` to `&[SynthSource]` (a requirement-level input
  type change, not just an implementation detail).

## Impact

- `ft-core/src/synth/`: new `source.rs` (`SynthSource` + `From` impls);
  `scaffold.rs` + `accrete.rs` input-type migration; their tests
  updated to build `SynthSource` directly.
- `ft-core/src/gather.rs`, `ft-core/src/recent.rs`: callers lower to
  `SynthSource` at the commit boundary (`GatherEntry` / `RecentEntry`
  keep their feed-specific fields; only the synth handoff changes).
- `ft/src/cmd/synth.rs`: `pick_paragraph` builds `SynthSource`
  directly (drops the best-effort blame `date` dance that only existed
  to fill `GatherEntry.date`).
- `ft/src/tui/notes_actions/`: new `paragraph_synth.rs` flow module
  mirroring `section_move.rs`'s shape (state enum + free-function key
  handlers returning a step outcome).
- `ft/src/tui/modal.rs`: new `ActiveModal::ParagraphSynth` variant +
  delegation.
- `ft/src/tui/tabs/graph/`: `y` binding + a tree-seeded entry mirroring
  `GraphMoveOuter::SourceFromTree`.
- `ft/src/tui/tabs/notes/mod.rs`: `y` binding + source-picker entry.
- `ft/src/tui/widgets/`: reuse `render_feed_split`; possibly a small
  helper for the range-adjust preview highlight (lines inside the
  effective range highlighted, lines outside dimmed).
- `docs/keybindings.md`: regenerated via `ft commands docs`.
- Tests: new `ft/src/tui/tests/` snapshot for the modal's steps;
  updated unit tests in scaffold/accrete/gather/recent/cmd-synth;
  the existing `synth_dirty_source` / `synth_untracked_source`
  behaviors are unchanged (the entry guard is additive defense).
