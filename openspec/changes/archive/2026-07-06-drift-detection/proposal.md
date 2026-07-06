# Proposal: drift-detection

## Why

The recurring cost of a link-based vault is vocabulary, not filing:
`[[onboarding]]`, `[[onboarding-flow]]`, and `[[new user onboarding]]`
silently split one concept across three names, and every tool that
counts references — review, ghosts, journal, related — undercounts by
three (exploration record, session 4). The *resolution* machinery
already exists and was verified: `ft notes rename "[[ghost]]" <note>`
merges a ghost into an existing note by rewriting every link, and
Related-section aliases handle "both names are legitimate." What's
missing is **detection**: no surface finds drifted siblings or says
which ones matter.

## What Changes

- **Core drift detector.** `ft_core::graph::drift::detect_drift(graph,
  vault)`: candidate pairs over all mentioned concepts (notes and
  ghosts), gated by **name similarity** (normalized token
  containment/overlap plus small edit distance), confirmed by
  **neighborhood overlap** (shared co-occurrence profile, reusing the
  `related` scoring machinery), penalized by **direct co-occurrence**
  (concepts appearing in the same paragraph are related-but-distinct,
  not drift), and ranked by combined distinct-paragraph mention weight
  so high-stakes splits surface first.
- **CLI report.** New `ft notes drift` (name provisional; session 5
  owns verbs): ranked pairs with mention counts and ghost markers,
  each with a ready-to-paste resolution — `merge: ft notes rename
  "[[lesser]]" "<keeper>"` when a ghost is involved, an alias
  suggestion (Related section) otherwise. `--limit`, `--json`,
  `--no-color`; empty result exits 0.
- **Out of scope:** any automatic merging (the report only suggests);
  a TUI drift flow (own change if the report proves useful); note↔note
  *content* merging (only alias advice is offered there — two real
  files can't be link-merged); renames (session 5).

## Capabilities

### New Capabilities

- `drift-detection`: the drift candidate model (signals, gating,
  ranking) and the CLI report surface.

### Modified Capabilities

<!-- none — purely additive; no existing requirement changes -->

## Impact

- `ft-core`: new `graph::drift` module; reads `ParagraphLink` edges
  and reuses `related::score_related` profiles for gated pairs only
  (bounds the O(n²) pair space to cheap token compares).
- `ft` CLI: new subcommand in `ft/src/cmd/notes.rs` + integration
  tests.
- Docs: `docs/guide/notes.md` (a drift section next to Ghosts) and a
  cross-reference from philosophy's "Keeping names honest".
- No TUI changes, no keymap changes, no `docs/keybindings.md` churn.
