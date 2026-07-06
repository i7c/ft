# Design: drift-detection

## Context

Grounding (2026-07-06): merge-by-rename already works for ghosts
(`plan_rename` rewrites ghost links to an existing note; dry-run
verified), Related-section aliases cover "keep both names", and
`related::score_related` already computes per-concept co-occurrence
profiles (same-paragraph +3, same-file +1). Capture-time completion
(ft.nvim / Obsidian) is the first drift defense per the user's
correction; this change is the after-the-fact net. Decisions taken on
standing recommendations (question timed out, flagged for review):
all pair kinds covered; CLI-only v1.

## Goals / Non-Goals

**Goals:**

- Find drifted siblings without the user knowing what to look for,
  ranked so the high-stakes splits come first.
- Every reported pair carries its resolution, ready to paste.
- Bounded cost: expensive scoring only for name-gated pairs.

**Non-Goals:**

- No automatic merging or writing of any kind — read-only report.
- No TUI surface (own change later if warranted).
- No content merging for note↔note pairs (alias advice only).
- No new dependencies (edit distance is a ~15-line inline DP).
- No renames of existing commands (session 5).

## Decisions

**D1 — concept universe.** Candidates are Note and Ghost nodes with at
least one distinct-paragraph mention (`ParagraphLink`, deduped by
paragraph — the same counting rule as review/ghosts). Zero-mention
notes can't drift-split anything and would only add pair noise.

**D2 — name similarity is the gate.** Normalize: lowercase, strip
`.md` and directories, split on whitespace/`-`/`_`/`/`, trim a
trailing `s` per token. Similarity = max of token *containment*
(|A∩B| / min(|A|,|B|) — makes "onboarding" vs "onboarding-flow" score
1.0), token Jaccard, and normalized Levenshtein on the joined string
(catches typo drift like "onbaording"). Pairs below a fixed threshold
(0.5) are dropped before any graph scoring, keeping the O(n²) phase
to cheap token math. Threshold is a module constant, not a flag —
tune with real vaults before exposing knobs.

**D3 — neighborhood overlap confirms, direct co-occurrence vetoes.**
For gated pairs only, build both concepts' co-occurrence profiles via
`score_related` (each side excluded from the other's profile) and
compute weighted Jaccard (Σ min / Σ max over shared concept scores).
Separately count paragraphs mentioning *both* — the anti-signal: true
duplicates rarely co-occur in one paragraph, so direct co-occurrence
divides the final score. Rationale: name similarity alone flags
legitimate neighbors ("project-a" / "project-b"); the neighborhood
signal separates same-thing from near-thing, and the co-occurrence
penalty separates drift from genuinely related concepts that are
written about together.

**D4 — ranking encodes stakes.** Final score multiplies the signal
product by `ln(1 + combined mentions)` so a 30-mention split outranks
two 2-mention ghosts (the user's "which links to look at"). The exact
formula is an implementation detail behind ordering-property tests;
the spec pins observable ordering, not coefficients.

**D5 — suggestion policy.** When at least one side is a ghost: suggest
`merge: ft notes rename "[[lesser]]" "<keeper>"`, folding the
lower-mention side into the higher (when exactly one side is a real
note, the note is always the keeper — a file beats a phantom).
Note↔note: suggest listing the lesser under the keeper's `## Related`
section (alias), noting that content merging is manual. The
suggestion is text; nothing executes.

**D6 — CLI shape.** `ft notes drift`: rows as

```
[[onboarding]] (31) ↔ [[onboarding-flow]]? (4)
  merge: ft notes rename "[[onboarding-flow]]" "onboarding"
```

(`?` marks the ghost side, review's grammar; counts are distinct
paragraphs). `--limit <n>`, `--json` (pair objects with both sides'
`{target, is_ghost, mentions}`, the three signal values, `score`, and
`suggestion`), `--no-color`. Empty: `no drift candidates found`,
exit 0.

## Risks / Trade-offs

- [False positives] Similar names with overlapping neighborhoods can
  still be legitimately distinct ("2025 planning" / "2026 planning").
  → Read-only report; the human decides. The co-occurrence penalty
  catches many; the rest is why nothing auto-executes.
- [O(n²) gate on big vaults] Thousands of concepts → millions of
  token comparisons. → Each compare is a few token-set ops; gate runs
  in memory with no I/O. Perf-gated tests can watch it; a blocking
  index (first-token buckets) is the escape hatch if ever needed.
- [score_related cost per gated pair] Two profile walks per surviving
  pair. → Survivors are few by construction (name gate); profiles
  could be memoized per concept if a vault produces many.
- [Threshold tuning] 0.5 is a guess. → Constant in one place,
  ordering tests don't depend on it tightly, revisit with real-vault
  experience before exposing a flag.

## Migration Plan

Additive, core → CLI. Nothing to migrate.

## Open Questions

- None blocking; the provisional command name (`ft notes drift`) is
  session 5's to finalize.
