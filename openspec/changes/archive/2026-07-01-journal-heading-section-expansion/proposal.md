## Why

The journal (CLI `ft notes journal` and the TUI Journal tab) only surfaces the *single* paragraph that begins at a heading line like `## Thoughts about [[Foo]]`. Sibling paragraphs in the same section — which carry the discussion but don't repeat the wiki link — are dropped, so the feed misrepresents what the user actually wrote. When a note links a target from its heading, the whole section is about that target, and the journal should reflect that.

## What Changes

- **Heading-link section expansion.** When a heading node has a `HeadingLink` edge to a journal target, every paragraph transitively owned by that heading (its `OwnsParagraph` children plus those of its `OwnsHeading`-descendant sub-headings — i.e. the section up to the next same-or-higher heading) is included in the journal, not just the paragraph beginning at the heading line. This composes with the existing direct-`ParagraphLink` match: a paragraph reachable both ways appears once.
- **One entry per paragraph, always.** Each sibling paragraph becomes its own `JournalEntry`, preserving the one-paragraph-per-entry mapping. Synth scaffolding therefore consumes the expanded entries with no separate change.
- **Per-paragraph dates unchanged.** Each entry's date stays the most-recent commit touching that paragraph's own line range (blame over `[line_start, line_end]`), so the feed remains reverse-chronological by paragraph recentness. Section-expansion does not introduce a shared/section date.
- **`matched` attribution by inheritance.** In multi-target mode, a paragraph included only via heading expansion has no direct `ParagraphLink` to a target; its `matched` set is the subset of `targets` that its owning-chain headings link to (instead of the empty set). Single-target mode is unchanged (`matched == vec![targets[0]]`).
- **Document-order tiebreak (final, non-overriding).** The feed's sort gains a final `line_start asc` tiebreak after `(date desc, source_title asc)`, so same-date co-located paragraphs read top-to-bottom. It never reorders across distinct dates, so paragraph recentness stays the dominant signal.
- **Preserved invariants.** Single-target self-exclusion (drop paragraphs whose `source_file` is the target's path), `## Related`-alias expansion, dedup, the in-window filter (per-paragraph line-range overlap), and the lazy `BlameCache` / `skipped_blame` behavior are all unchanged. The trigger is specifically the `HeadingLink` edge kind (a link written *inside* heading text); anchored links *targeting* a heading from elsewhere (`[[Foo#Bar]]`) remain handled by `mentions_of` and do not trigger section expansion.

## Capabilities

### New Capabilities
<!-- None. This change reuses the existing `OwnsHeading`/`OwnsParagraph` containment graph (and the `note_paragraphs` traversal) introduced by `graph-contents-semantics`; no new graph primitive is needed. -->

### Modified Capabilities
- `notes-journal`: The "Journal matching via ParagraphLink edges" requirement gains heading-link section expansion — a paragraph is also included when a heading in its owning chain has a `HeadingLink` to a target, with `matched` inherited from the heading and a final `line_start` document-order tiebreak added to the sort.

## Impact

- **Code:** `ft-core/src/journal.rs::build_journal` — add a heading-expansion pass to paragraph collection (iterate target `HeadingLink` sources → expand each to its section's paragraphs via `Graph::note_paragraphs`); extend multi-target `matched` computation to attribute expanded paragraphs via their owning chain; add `line_start asc` to the sort comparator. The CLI (`ft/src/cmd/notes.rs::run_journal`) and TUI (`ft/src/tui/tabs/journal.rs::JournalTab::rebuild_journal`) require no changes — both funnel through `build_journal`, and the synth scaffold (`ft-core/src/synth/scaffold.rs`) already consumes `entries` as-is.
- **Specs:** Only `notes-journal` requirements change. `synth-notes` is touched in data-flow only (more entries flow in) with no requirement-level behavior change — explicitly out of scope per the archived `graph-contents-semantics` "Scaffold content sourcing" requirement (sections stay ordered by journal date desc, `section_text` shape unchanged).
- **Tests:** New unit tests in `ft-core/src/journal.rs` (heading expansion incl. sub-headings, `matched` inheritance, dedup vs direct `ParagraphLink`, self-exclusion interaction, ghost-target heading links, the `line_start` tiebreak) and an integration test in `ft/tests/notes_journal.rs` mirroring the existing multi-link test shape.
- **Risk:** Low — additive match predicate; the only behavioral change for existing users is *more* entries appearing when a heading links a target, which is the intended fix.
