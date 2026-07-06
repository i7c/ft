# Proposal: citation-visibility

## Why

Nothing in ft ever shows whether a paragraph has already been woven
into a synthesis note. The machinery exists — `synth scaffold`/`grow`
dedup against existing callouts via `accrete::filter_missing`, and the
watermark cuts at the last synth — but it all happens silently at plan
time. The result (exploration record, session 2): journal and history
re-show the same paragraphs forever, so re-triage cost grows with
vault age, and the user working a journal toward a note cannot see
what's already in the note versus what's missing. This change makes
citation state visible everywhere paragraphs are fed back.

## What Changes

- **New ft-core citation index.** Scan `ft-synth: true` notes, parse
  their `[!ft-source]` callouts, and build a lookup from paragraph
  identity to the notes that cite it. Identity is content-based,
  mirroring `filter_missing`: *cited* = a callout pins the same source
  path with the exact same body. A third state, *cited-stale*, marks
  paragraphs where a callout pins the same source path with an
  overlapping line range but a different body (edited since cited).
- **CLI feed badges.** `ft notes journal` and `ft notes history` mark
  each entry `cited: <note>` (or `cited*` for stale) in text output,
  add a `cited_in` array (with per-citation staleness) to `--json`,
  and gain an `--uncited` filter that keeps only entries not cited
  anywhere (stale counts as uncited for filtering).
- **TUI feed badges.** Journal and History tab rows render the same
  badge; a new toggle command per tab shows only uncited entries.
  Citation data rides the shared `GraphSnapshot` rebuild.
- **Note-context mode (TUI).** When a target synth note is in play —
  entering the existing `s` (append-to-existing) flow, or opening a
  journal from a synth note's `ft-synth-targets` — badges switch from
  global to note-local: *already in this note* vs *missing from it*,
  computed live with the same matching as `filter_missing`.
- **Deliberately deferred:** a dismiss/"seen, irrelevant" mechanism.
  It is the only piece requiring durable non-derivative state; the
  window-shaped sweeps age noise out naturally and the uncited filter
  focuses sessions. Revisit if uncited feeds still feel
  haystack-shaped in practice (recorded in the exploration doc).

## Capabilities

### New Capabilities

- `citation-index`: the ft-core index from paragraph identity to
  citing synth notes — build, lookup semantics (cited / cited-stale /
  uncited), and its integration points (CLI per-invocation build, TUI
  GraphSnapshot).

### Modified Capabilities

- `notes-journal`: ADDED requirements — cited badge in text output,
  `cited_in` in JSON, `--uncited` filter. Existing feed semantics
  unchanged.
- `notes-history`: ADDED requirements — same three additions as
  notes-journal. Existing feed semantics unchanged.
- `journal-tui-tab`: ADDED requirements — row badge, uncited-only
  toggle command, note-local badges when a target synth note is in
  context. Existing behavior unchanged.
- `history-tui-tab`: ADDED requirements — row badge and uncited-only
  toggle command. Existing behavior unchanged.

## Impact

- `ft-core`: new module (e.g. `synth::citations`), reusing
  `callout::parse` and the `filter_missing` matching semantics; no
  changes to existing `build_journal`/`build_history` signatures —
  callers annotate entries via the index.
- `ft` CLI: `ft/src/cmd/notes.rs` (journal/history flags + rendering),
  output snapshots.
- TUI: Journal + History tabs (row rendering, new `<tab>.toggle-uncited`
  commands + keymap rows → regenerate `docs/keybindings.md`), synth
  append flow (note-local badge mode). New `TestBackend` snapshots.
- Docs: `docs/guide/synthesis.md` and `docs/guide/notes.md` gain the
  badge/filter story.
- Perf: callout parsing per synth note at scan/snapshot time — synth
  notes are few; no cache in v1 (`.ft/cache/` precedent exists if this
  ever measures slow).
