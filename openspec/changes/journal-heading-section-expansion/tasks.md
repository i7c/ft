## 1. Heading-expansion collection in `build_journal`

- [ ] 1.1 In `ft-core/src/journal.rs::build_journal`, extend the candidate-paragraph collection: after the existing direct-match pass (which keeps `Paragraph` sources from `mentions_of`), add an expansion pass over `Heading` sources. For each `Heading` source `H` yielded by `mentions_of(target)` for any target in the resolved `target_set`, call `Graph::note_paragraphs(H)` and insert every returned paragraph `NoteId` into the candidate set (deduped via the existing `seen_paragraph` HashSet). Do not insert `H` itself unless it is also a `Paragraph` source (it isn't — `H` is a `NodeKind::Heading`).
- [ ] 1.2 Verify the heading-paragraph (Fork A2: the paragraph beginning at the heading line) is still included exactly once — it appears both as a direct `Paragraph` source (its own `ParagraphLink`) and as a member of `note_paragraphs(H)`. Confirm dedup yields a single candidate.
- [ ] 1.3 Confirm self-exclusion still applies to expanded paragraphs: in single-target mode, paragraphs whose `source_file` equals the target's path are dropped after expansion, exactly as direct-matched ones are (no special-casing needed — the existing `self_path` check runs on every candidate).

## 2. `matched` attribution

- [ ] 2.1 Add a private helper `heading_chain_targets(graph, paragraph_id) -> HashSet<NoteId>` in `journal.rs` that: finds the paragraph's nearest `OwnsParagraph` container; if it is a `Heading`, walks that heading and its `OwnsHeading` ancestors up to the note, collecting every target reachable via outgoing `EdgeKind::HeadingLink` edges (mapped to note identity through `Graph::link_target_note`). Returns the empty set when the container is the note or no heading links a target.
- [ ] 2.2 In the multi-target `matched` computation, apply the "direct wins" rule: compute `direct` from the paragraph's own outgoing `ParagraphLink` edges (as today); if `direct` intersected with `targets` is non-empty, use it; otherwise fall back to `heading_chain_targets` intersected with `targets` (preserving caller order). Single-target mode stays `vec![targets[0]]`.
- [ ] 2.3 Verify the "Direct- and expansion-matched paragraph appears once" scenario: a paragraph with its own `ParagraphLink` to a target that is also under a linking heading gets `matched` from its direct edge, not from inheritance.

## 3. Sort tiebreak

- [ ] 3.1 Extend the `entries.sort_by` comparator in `build_journal` with a final `line_start` ascending tiebreak after `(date desc, source_title asc)`. Confirm it never reorders entries that differ in date or title (it only breaks previously-arbitrary ties).

## 4. Unit tests (`ft-core/src/journal.rs`)

- [ ] 4.1 Add a `make_vault_with_history` variant (or a focused fixture) with a heading `## Thoughts about [[Foo]]` followed by paragraphs A, B, C under it and a `## Next section` paragraph D. Assert the journal for `Foo` includes A, B, C and excludes D (covers the "Heading link expands to all sibling paragraphs" + "Expansion includes paragraphs under nested sub-headings" scenarios — add a `### Sub-point` case for the nested assertion).
- [ ] 4.2 Add a test asserting each expanded paragraph keeps its own per-paragraph date: commit paragraphs on different days, assert entries' `date` fields match each paragraph's own blame date (covers "Expanded paragraph keeps its own per-paragraph date").
- [ ] 4.3 Add a multi-target test (`--link Foo --link Bar`) where a paragraph under a `## About [[Foo]]` heading has no direct link; assert its `matched == vec![Foo]` (covers "Expanded paragraph matched inherited from the linking heading").
- [ ] 4.4 Add a test for the dedup + direct-wins scenario: a paragraph with its own `ParagraphLink` to `Foo` that also sits under a `## ... [[Foo]]` heading appears once with `matched` from its direct edge (covers "Direct- and expansion-matched paragraph appears once").
- [ ] 4.5 Add a single-target self-exclusion test: target note `Foo` contains `## Notes about [[Foo]]` + paragraphs; assert none of `Foo`'s own paragraphs appear (covers "Single-target self-exclusion still drops the target note's own paragraphs").
- [ ] 4.6 Add a ghost-target test: heading `## About [[Phantom]]` with no `Phantom.md`; assert the section paragraphs appear for the `Phantom` ghost target (covers "Heading link to a ghost target expands its section").
- [ ] 4.7 Add a sort-tiebreak test: three same-date same-title paragraphs with `line_start` 5/9/13; assert output order is A, B, C (covers "Same-date same-title paragraphs ordered by document position").
- [ ] 4.8 Add a negative test: a body paragraph `[[Foo#Bar]]` targeting a heading of `Foo` includes that paragraph (direct match) but does NOT expand `Foo`'s `## Bar` section (covers "Anchored link targeting a heading does not trigger expansion").

## 5. Integration tests (`ft/tests/notes_journal.rs`)

- [ ] 5.1 Add a CLI integration test mirroring the existing multi-link test shape: a fixture vault with a heading-sited `[[Target]]` link and sibling paragraphs across two commits; assert `ft notes journal Target` (table output) surfaces all sibling paragraphs in reverse-chronological order with per-paragraph dates.
- [ ] 5.2 Add a CLI `--json` integration test asserting the expanded sibling paragraphs each appear as separate rows with correct `matched` (single-target → `[Target]`).

## 6. Snapshot + build invariants

- [ ] 6.1 Regenerate any `insta` snapshots in `ft-core/src/journal.rs` tests affected by the new expansion behavior (only regenerate where the expansion genuinely changes expected output; do not blanket-update).
- [ ] 6.2 Run the full build-invariant suite and keep all five clean: `cargo build --release`, `cargo test --workspace`, `cargo clippy --workspace --tests -- -D warnings`, `cargo fmt --check`, `cargo run --release -q -- commands docs --check`.

## 7. Archive readiness

- [ ] 7.1 Confirm `openspec validate journal-heading-section-expansion` passes after implementation, then archive the change via the openspec archive flow (syncs the `notes-journal` delta into the canonical spec).
