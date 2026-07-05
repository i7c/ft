# Tasks: note-flow-story-rewrite

## 1. Groundwork

- [ ] 1.1 Read the current README.md, docs/guide/philosophy.md,
  docs/guide/index.md, and docs/guide/synthesis.md end to end; list
  every factual claim and cross-link that must survive the rewrite
  (per design non-goal: no documented fact is lost)
- [ ] 1.2 Verify the README demo commands (`ft review --since 7d`,
  `ft notes journal --link …`, `ft synth scaffold …`) against a
  fixture vault so the rewritten demo shows real flags and output
  shape (design D6)
- [ ] 1.3 Confirm the ft.nvim repository URL and exact plugin name
  with the user; if unavailable, proceed mention-only (design D5)

## 2. README rewrite

- [ ] 2.1 Rewrite README.md in the D1 order: problem hook → how it
  works in four beats (write anywhere / name concepts / resurface via
  pull and sweep / consolidate when a topic earns it) → reframed
  terminal demo → what ft is / is not + compatibility table
  (shortened) → tasks and timeblocks as adjacent features → guide
  pointers
- [ ] 2.2 Check README against the spec scenarios: first screen tells
  the problem with no feature inventory; all three thesis claims
  present in concise form; pull/sweep framing honest; no "ritual"

## 3. Philosophy rewrite

- [ ] 3.1 Rewrite docs/guide/philosophy.md in the D2 order: problem →
  local vs. global decisions → retrieval severed from filing → the two
  triggers mapped onto today's command names → links-vs-search
  objection posed and answered → keeping names honest (ft.nvim and
  Obsidian completion, Related-section aliases, `notes rename` as
  merge) → where notes live (daily-note sections, paragraph
  granularity, git as memory, commit cadence, `ft git sync`)
- [ ] 3.2 Re-attach the engineering tail (companion-not-replacement,
  CLI/TUI split, atomic writes, one-way-to-spell, deliberately-doesn't,
  defaults) after the story, condensed where it now repeats the story
- [ ] 3.3 Check philosophy.md against the spec scenarios: objection
  visible in headings/opening sentences; ft.nvim named; aliases
  presented as the drift answer; commit-cadence consequence stated
  with `ft git sync` pointed at

## 4. Guide sweep and link integrity

- [ ] 4.1 Sweep "ritual" from docs/guide/index.md and
  docs/guide/synthesis.md, reframing those passages with the
  pull/sweep/consolidate vocabulary; update index.md's description of
  the philosophy page if its summary no longer matches
- [ ] 4.2 Run `rg -i ritual README.md docs/guide/` and confirm zero
  matches (spec acceptance check)
- [ ] 4.3 Resolve every relative link in the four touched files
  against the repo tree; grep the repo for inbound links/anchors to
  any renamed heading and fix referrers

## 5. Verification

- [ ] 5.1 Run the workspace checks that could see doc-adjacent
  breakage (`cargo test --workspace` for doc-string tests,
  `cargo run --release -q -- commands docs --check`) and confirm green
- [ ] 5.2 Read both rewritten docs top to bottom in one sitting for
  voice (dry, concrete, no marketing tone) and for consistency of the
  capture → resurface → consolidate vocabulary across README,
  philosophy, index, synthesis
