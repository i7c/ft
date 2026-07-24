## Context

The synth pinning engine (`ft-core::synth`) already does everything
this feature needs: it builds `[!ft-source]` callouts pinned to HEAD,
dedups on append, verifies against the git blob. What's missing is a
*source-driven* entry — today every synth input comes from a feed
(gather = link-targeted paragraphs; recent = time-windowed paragraphs)
that decides which paragraphs to surface. The user has no way to open
an arbitrary note and pick specific paragraphs by hand.

Two existing flows each cover half of the target shape:

- **Section-move modal** (`ft/src/tui/notes_actions/section_move.rs`):
  the right *shape* — a step state machine (source pick → heading
  multi-select → target pick → compose → commit) driven as an
  `ActiveModal`. But its unit is the *heading section* and its commit
  *moves* (deletes from source).
- **Gather/Recent send-to-synth** (`ft/src/tui/tabs/{gather,recent}.rs`
  `commit_send`): the right *commit* — `plan_synth_scaffold` +
  `apply_synth_scaffold` + `$EDITOR` handoff. But its inputs are
  pre-computed feed entries, and the user cannot pick paragraphs.

The new flow is the section-move shape with a paragraph-multi-select
step (instead of heading-multi-select) and the gather commit. The
honest input type already exists on the *output* side
(`ProtectedSection` has exactly `source_path`/`line_start`/
`line_end`/`body`); only the *input* type has been borrowing
`GatherEntry`, which carries two feed-only fields (`date`, `matched`)
that source-driven picks would have to fabricate.

Constraints baked into the existing engine that shape this design:

1. **Pins are to HEAD.** `plan_synth_scaffold` refuses dirty/untracked
   sources (`SynthDirtySources`) so the HEAD blob reproduces the pinned
   body. The new flow inherits this and adds an *entry* guard so the
   user doesn't reach the commit before discovering a dirty tree.
2. **Dedup is by `(source_path, body)`.** `accrete::filter_missing`
   drops entries already pinned; re-picking the same paragraph into the
   same note is a no-op + toast. This is desirable and free.
3. **Paragraph extraction already exists.** `markdown::extract_paragraphs`
   returns `Paragraph { line_start, line_end, text }` with tested
   boundary rules (blank line, heading, rule separator; skips
   frontmatter / fenced code). The new step consumes this directly.

## Goals / Non-Goals

**Goals:**
- Source-driven copy-to-synth: pick a note → pick paragraphs → pin into
  a target note as protected `[!ft-source]` callouts. No feed required.
- Shrink-only range-adjust so a paragraph that bundles two topics can
  be split at pin time without editing the source note.
- Graph-tab entry (seed from the focused Note node) and Notes-tab entry
  (source-note picker first).
- Reuse the gather commit path (plan/apply + editor handoff) and the
  split-pane paragraph view (`render_feed_split`).
- Unify the synth input type: `SynthSource` as the honest 4-field
  boundary, replacing `GatherEntry` at the scaffold/accrete seams.

**Non-Goals:**
- **Move** (deleting paragraphs from the source). This is copy-synth;
  section-move already handles structural moves and is a different
  mental model. Pinning + deleting would also make the source dirty,
  fighting the clean-tree invariant.
- **Grow beyond paragraph bounds.** Multi-select spans adjacent
  paragraphs; growing a single pick past a blank line defeats the
  paragraph model and is not needed.
- **Compose/reorder step.** Sections append in selection order;
  reordering happens in `$EDITOR` after handoff. (Section-move has a
  compose step because it shifts heading levels; synth callouts don't.)
- **A no-git / best-effort pin variant.** Out of scope; the clean-tree
  guard makes this unnecessary for committed vaults.
- **CLI command.** The existing `ft notes synth` CLI is unaffected.
  This is a TUI-only flow. (A future CLI `--from <path>:<line>` style
  manual pick could reuse `SynthSource`, but is not built here.)
- **Adjusting unselected picks persisting across toggles.** Adjust
  state lives on the pick; toggling selection off discards the adjust
  (see Decisions).

## Decisions

### D1. Copy, not move — and pin before any source mutation
**Decision:** The flow copies paragraphs into the target; the source is
untouched. Pins go to HEAD.

**Rationale:** The synth philosophy is "the synth note is a curated
quote of source material; sources stay canonical." Move would (a)
dirty the source immediately, breaking the clean-tree invariant the
engine already enforces, and (b) point the pin at paragraphs that no
longer exist at HEAD, muddying future re-runs. Copy keeps re-pinning
idempotent (dedup no-op) and verify-evergreen (historical blob stays).

**Alternatives considered:** *Move with pin-then-delete-then-commit.*
Rejected: it's two extra git operations the user must perform, the
source's working tree is dirty mid-flow, and it duplicates
section-move's structural move for no gain.

### D2. Entry guard: refuse a dirty working tree before opening the modal
**Decision:** Both entry points check `git::status(repo.root()).is_clean()`
first. If dirty → toast `synth needs a clean working tree (commit or
stash first)` + abort; the modal never opens.

**Rationale:** The planner re-checks per-source dirty/untracked at
commit time (`SynthDirtySources`), but discovering a dirty tree after
navigating the source + paragraph + target steps is frustrating. The
entry guard is cheap (one `git status`) and fails fast. It is additive
defense — the planner check stays.

**Alternatives considered:** *Only the planner check.* Rejected: late
failure. *Only the entry check.* Rejected: defense-in-depth — a
`git pull`/external edit between entry and commit must still be caught.

### D3. New `ActiveModal::ParagraphSynth` variant, driven like section-move
**Decision:** A new modal variant `ActiveModal::ParagraphSynth` holds a
`ParagraphSynthState` step enum. The flow module
(`ft/src/tui/notes_actions/paragraph_synth.rs`) owns the state enum +
free-function key handlers returning a `SynthStep` outcome
(`Stay`/`Transition`/`Finished`/`NotHandled`) — exactly the section-move
module's shape. `Modal::handle_event` wraps the handler; `Modal::render`
renders the active step.

**Rationale:** This is the documented pattern for multi-step flows
(see `docs/architecture.md` §"Modal driver"): new flow → new
`ActiveModal` variant, not a per-tab `Option<...>` field. Section-move
is the reference impl and the closest analogue (step state machine,
source/target pickers, shared `VaultFilePickerSource`).

**Alternatives considered:** *Tab-resident state like `ResliceState`.*
Rejected — the modal driver is the sanctioned pattern for new flows;
reslice is explicitly called out as the "one remaining" pre-modal
holdout and new flows should not follow it. *A second
`SectionMoveState` mode.* Rejected — different commit semantics (copy
vs move) and different unit (paragraph vs heading) would entangle two
flows in one state machine.

### D4. Paragraph multi-select step with `render_feed_split` + range-adjust preview
**Decision:** Step 2 renders a top list (one row per paragraph: marker
+ line range + a short preview) and a bottom preview pane, via
`render_feed_split`. `j/k` move, `Space` toggles, `Enter` → target
pick. The focused pick's range-adjust keys (`[`/`]`/`r`) operate and
the preview reflects the live effective range: lines inside the
adjusted range are highlighted, lines trimmed away are dimmed. The
preview header shows `L<orig> (adj: L<effective>)`.

**Rationale:** `render_feed_split` is already the shared list/preview
widget for paragraph feeds (gather + recent) with cursor-follow +
scrollbar. Reusing it keeps the visual vocabulary uniform and gets
scrolling for free. The split layout makes the shrink result visible
without a separate compose step — the user sees exactly what will be
pinned before committing.

**Alternatives considered:** *A single scrolling list of full
paragraphs (no preview pane).* Rejected: long paragraphs would push
the list around and hide the range-adjust result. *A modal-within-modal
range editor.* Rejected: overkill; inline preview is enough.

### D5. Range-adjust is shrink-only, per focused pick, floor of 1 line
**Decision:** Each pick carries optional `top_trim`/`bot_trim` (u32,
default 0). Effective range =
`(line_start + top_trim) ..= (line_end - bot_trim)`. `[` increments
`top_trim`; `]` increments `bot_trim`; `r` resets both to 0. Both clamp
so the range always contains ≥ 1 line. The pin's `body` is re-sliced
from the source content at the effective range, *not* the original
paragraph text. The hash + verify use the adjusted body/range.

**Rationale:** Shrink handles the stated need (a paragraph bundling two
topics → split at pin time). Grow is unnecessary because adjacent
paragraphs are independently multi-selectable. Keeping the anchor to a
single paragraph means every pin is a sub-range of a real semantic
unit — never a cross-paragraph blob, which would produce
decontextualized snippets. Floor-of-1 prevents degenerate empty pins.

**Alternatives considered:** *Free line-range selection (no paragraph
anchor).* Rejected: maximum flexibility but maximum cognitive load and
furthest from the "topic" mental model. *Grow allowed.* Rejected (see
Non-Goals). *Range state stored on a separate `Vec` keyed by index.*
Rejected: storing on the pick (or a parallel `BTreeMap<usize, Adjust>`)
keeps the selected-set the single source of truth for "what commits."

### D6. Adjust operates on the focused pick regardless of selection; only selected picks commit
**Decision:** `[`/`]`/`r` act on whichever pick has focus, whether or
not it is currently selected (`Space`-toggled). Only *selected* picks
are carried to the target step. An adjusted-but-unselected pick shows
its adjust in the preview (useful for "what if I select this one?")
but is not pinned.

**Rationale:** More fluid — the user can audition a trim on a
candidate paragraph before committing to selecting it. Carrying adjust
state for unselected picks is cheap (a `BTreeMap<usize, Adjust>`).
**Open question:** whether to clear an unselected pick's adjust when
focus leaves it (keeps the map small) — see Open Questions.

**Alternatives considered:** *Adjust only when the focused pick is
selected.* Rejected: forces a toggle-then-adjust-then-maybe-deselect
dance.

### D7. Reuse gather's target pick + commit; no compose step
**Decision:** Step 3 reuses the gather tab's send-to-synth target split:
`s` → existing-note fuzzy picker; `S` → create-new (folder → title →
template + var prompts, via the existing `NewTargetCreating`-style
sub-flow). Commit calls `plan_synth_scaffold` + `apply_synth_scaffold`,
marks the target as synth (`mark_note_as_synth`) when appending to an
existing non-synth note, and hands off to `$EDITOR`. Sections append in
selection order.

**Rationale:** The target + commit machinery is already correct and
tested; rebuilding it would duplicate the dedup, watermark, and
frontmatter logic. No compose/reorder step because, unlike section-move
(which shifts heading levels and reorders structural units), synth
callouts are flat append-only blocks the user reorders trivially in the
editor.

**Alternatives considered:** *A compose step mirroring section-move.*
Rejected (see Non-Goals). *A new target-only flow.* Rejected —
duplicates the create-from-template sub-flow already in section-move
and gather.

### D8. Unified `SynthSource` input type replacing `GatherEntry` at the seams
**Decision:** New `ft-core/src/synth/source.rs`:

```rust
pub struct SynthSource {
    pub source_path: PathBuf,
    pub line_start: u32,
    pub line_end: u32,
    pub body: String,
}
impl From<&GatherEntry> for SynthSource { /* copy the 4 fields */ }
impl From<&RecentEntry> for SynthSource { /* copy the 4 fields */ }
```

`plan_synth_scaffold(vault, target, &[SynthSource])` and
`accrete::filter_missing(existing, Vec<SynthSource>)` migrate.
`GatherEntry`/`RecentEntry` keep their feed-only fields (`date`,
`matched`) — only the *synth handoff* changes. The new paragraph-synth
flow builds `SynthSource` natively (no `date`/`matched` to fabricate).
`cmd/synth.rs::pick_paragraph` builds `SynthSource` directly and drops
the best-effort blame `date` computation that existed solely to fill
`GatherEntry.date`.

**Rationale:** The output side (`ProtectedSection`,
`ParsedCallout`) already has the honest 4-field shape; the input was
the lie. `SynthSource` is the natural boundary. The `From` impls make
the migration mechanical (`.into()` at each call site) and keep feed
callers honest about which fields are feed-specific.

**Alternatives considered:** *Reuse `GatherEntry`, fill `date` with
today and `matched = vec![]`.* Rejected by the user: the cleaner
unified structure is preferred over a 2-field lie the planner ignores.
*Generalize `GatherEntry` with an enum.* Rejected — couples feed
concerns into the synth input.

### D9. Graph-tab entry mirrors `GraphMoveOuter::SourceFromTree`
**Decision:** On the Graph tab, `y` with a Note node focused posts
`OpenModal(ActiveModal::ParagraphSynth(...))` seeded directly into the
paragraph-multi-select step (source = focused node's path), after the
clean-tree guard. No confirm dance (unlike move's `m`-then-`m`).

**Rationale:** The source is unambiguous on the Graph tab (the focused
note), so a confirm step adds friction. `GraphMoveOuter::SourceFromTree`
is the precedent for tree-seeded entry. The Notes tab has no "focused
note," so it opens the source picker first.

**Alternatives considered:** *A confirm-then-pick dance like move.*
Rejected — move needs it because its semantics (move vs move-into) are
ambiguous; synth's source is the focused note, full stop.

## Risks / Trade-offs

- **[Risk] Dirty tree develops mid-flow (external edit / git pull
  between entry guard and commit).** → Mitigation: the planner's
  per-source `SynthDirtySources` check at commit time remains (defense
  in depth); the entry guard is the fast-fail, not the only check.
- **[Risk] Shrink produces a body whose hash differs from any
  historical paragraph, so future `reslice`/`repair` can't relocate it
  by body match.** → Mitigation: acceptable — a manually trimmed pin is
  intentionally a custom range; `reslice` operates on *existing* pins
  (it re-derives from the source file), and `verify` (the common op)
  keys on the pinned commit+range, not body search. Documented as a
  known limitation in the spec.
- **[Risk] Adjust-state-on-unselected-picks grows unbounded or
  confuses users (adjusting something that won't be pinned).** →
  Mitigation: the preview clearly shows the *effective* range and the
  list row shows selection state separately; adjust is cheap to store.
  Open question D6 follow-up may clear adjust on focus-leave.
- **[Risk] Refactor ripple: every scaffold/accrete test must rebuild
  `SynthSource` instead of `GatherEntry`.** → Mitigation: the `From`
  impls mean most tests can use `(&entry).into()`; only the
  constructor helpers change. This is a known, budgeted test-ripple
  (flagged per the AGENTS.md "signature changes on core APIs" note).
- **[Trade-off] No compose step means sections land in selection
  order, which may not be the desired final order.** → Accepted: the
  editor handoff is the reorder surface; a compose step would
  duplicate section-move's complexity for flat callout blocks.
- **[Trade-off] Shrink-only means a paragraph that needs *both* halves
  pinned as separate callouts requires two picks (select, trim, then
  re-select+trim the other half on a fresh pick).** → Accepted: rare;
  the multi-select + reset flow handles it, just not in one keystroke.

## Open Questions

- **D6 follow-up:** should an unselected pick's adjust state be
  cleared when focus leaves it (smaller map, less "stale" adjust
  confusion), or preserved (re-auditionable)? Lean: preserve, with the
  map keyed by paragraph index; clear on flow exit. Decide at
  implementation time against the snapshot test.
- **Preview highlight helper:** reuse `render_feed_split`'s body
  rendering as-is (no per-line highlight), or add a small helper that
  highlights the effective-range lines and dims the trimmed ones? The
  latter is much clearer for range-adjust UX. Lean: add the helper
  (a `render_split` variant or a body-line styler), since the whole
  point of the split pane is to show the adjust result. Decide at
  implementation; the spec requires the effective range be *visible*,
  not a specific rendering.
