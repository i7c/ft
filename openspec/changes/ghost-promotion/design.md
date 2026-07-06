# Design: ghost-promotion

## Context

Grounding (2026-07-06): the `ghosts` builtin preset exists but prints
an unranked, countless list; the DSL's `indegree` is equality-only and
`graph query` has no sort flag; the graph tab already promotes ghosts
via `create-blank` / `create-from-template` (note created at the
ghost's exact name, so all links resolve with zero rewriting);
`ParagraphLink` edges (paragraph → ghost, one per occurrence) make
distinct-paragraph counting a pure graph read. Decisions taken with
the user's standing recommendations (question timed out, flagged for
review): dedicated `ft notes ghosts` CLI command; scaffold-seeded
promotion included.

## Goals / Non-Goals

**Goals:**

- One counting rule, defined once in core, shared by CLI list, TUI
  badges, and ordering.
- The existing `ghosts` preset becomes the ranked view everywhere it
  already appears (CLI walk output, TUI graph tab) without new query
  syntax.
- Promotion in three flavors on a ghost row, all one flow deep:
  blank (exists), template (exists), scaffold-seeded (new).

**Non-Goals:**

- No DSL ordering syntax (`sort by …`) — that is a query-language
  feature with its own design space.
- No changes to `review` (stays window-shaped; its `?` marker is
  untouched).
- No CLI seeded-promotion command — `ft synth scaffold <path> --link
  "[[ghost]]"` already is one; document it instead.
- No renames (session 5).

## Decisions

**D1 — counting rule: distinct paragraphs.** A ghost's rank is the
number of *distinct paragraph nodes* with a `ParagraphLink` edge to
it — the same dedup rule `review` uses (three mentions in one
paragraph count once). Not raw in-degree (occurrence multiset), not
note-level links. Rationale: the number must mean the same thing in
`review`'s window rows and the vault-wide list, or users will see the
two disagree. Alternative (raw edge count) rejected for that reason.

**D2 — core lives in `ft_core::graph::ghosts`.**
`rank_ghosts(graph) -> Vec<GhostRank { id, raw, mentions }>`, sorted
mentions-desc then raw-asc. Callers filter/limit. The TUI reads it
through the shared snapshot's graph (no new snapshot field needed —
it's a cheap derivation, computed where needed per generation).

**D3 — ghost ordering in walks.** Where the walk's deterministic sort
currently orders ghosts alphabetically within their kind group
(`graph::query::eval::child_sort_key`, and root selection order),
ghosts order by `(mentions desc, name asc)` instead. This changes
output order for any existing ghost query — deterministic still, but
different; existing snapshots with multiple ghosts get reviewed, not
blind-accepted. Alternative (only the TUI reorders) rejected: the CLI
`--preset ghosts` walk should be the same ranked view or the feature
is half-shipped.

**D4 — CLI shape.** `ft notes ghosts`: default table of
`(N) [[ghost]]` rows (review's row grammar, so the two read as one
family), `--limit <n>`, `--min-mentions <n>` (default 1), `--json`
(array of `{target, mentions}`), standard `--no-color`. Empty result
prints `no ghosts in the vault` and exits 0.

**D5 — TUI count badge.** Ghost rows in the graph tab render
`… (N)` after the label, N from the same core ranking (computed once
per snapshot generation, held in the tab's derived view state — no
per-frame edge counting). Zero-mention ghosts cannot exist (a ghost
node exists only because something links to it), so no `(0)` case.

**D6 — seeded promotion is the existing send-to-synth machinery
pointed at a ghost.** `graph.promote-ghost` (proposed key: `P`,
subject to `ft commands check-keymap`) on a ghost row: build the
ghost's journal entries (`build_journal`, single target — requires
git; toast on failure like the Journal tab does), plan/apply synth
scaffold to `<ghost raw>.md` with `ft-synth-targets: ["[[ghost]]"]`,
request graph refresh, open editor at the new note. The ghost node
disappears on refresh because the note now exists — which is the
point. Dirty-source guard: `plan_synth_scaffold` already refuses when
source files are dirty; surface that error as a toast.

## Risks / Trade-offs

- [Snapshot churn from D3] Ghost reordering may touch existing walk /
  TUI snapshots. → Budgeted in tasks; review each diff.
- [Ranking cost] `rank_ghosts` is O(edges) per call; the TUI calls it
  per generation, not per frame. → Fine at vault scale; perf tests
  are gated anyway.
- [Seeded promotion needs git] `build_journal` needs blame dates. →
  Same constraint and same toast pattern as the Journal tab; blank /
  template promotion remains available without git.
- [`P` key collision] The graph keymap is dense. → Verify with
  `ft commands check-keymap`; pick another chord if taken.

## Migration Plan

Additive; core → CLI → TUI order so the ranked list works headlessly
before the graph-tab work lands.

## Open Questions

- Whether root-selection order (as opposed to child order) flows
  through `child_sort_key` or a separate path in `select()` — resolve
  at implementation; the requirement is the observable ordering, not
  the mechanism.
