# Note-flow reframe — exploration record

Date: 2026-07-05. Status: planning umbrella for multiple dedicated sessions.

This document captures a critique session on ft's core note-taking
thesis. It exists so that each follow-up session can start from shared
context instead of re-deriving it. **Workflow:** each session below
starts by reading its section here, digs deeper, then runs
`openspec-propose` to become a real change. When a session's change is
archived, mark it done here. Nothing in this file is implementation —
hypotheses are starting points, not decisions, unless marked **locked**.

---

## The sharpened thesis (locked — this is the frame for everything below)

ft's positioning today: capture must be instantaneous and unorganized
("add anywhere"); organization happens later via on-demand, focused
tools (history, journal, review, related, synth) instead of a scheduled
sorting ritual.

The critique sharpened this into three claims the current docs don't
make:

1. **Local vs. global decisions.** "Add anywhere" is not
   zero-organization capture — dropping `[[concept]]` into a paragraph
   *is* organization. What ft removes is the *global* decision
   ("where does this go?" — requires the whole taxonomy in your head,
   fails under time pressure) and keeps only the *local* one ("what is
   this about?" — you already know, costs seconds). ft moves
   organization from global decisions to local ones.

2. **Retrieval severed from filing.** Because journal/related make the
   unsorted pile retrievable *by concept*, filing is not accelerated —
   it is **optional**. Retrieval no longer depends on sorting ever
   happening. Synthesis becomes optional compression applied only to
   topics that prove they matter, not maintenance you owe the system.

3. **Two triggers, both legitimate.** *Pull* ("I need everything on X
   now — meeting in 10 minutes") is served by journal/related/synth and
   is genuinely on-demand. *Sweep* ("what accumulated?") is served by
   history/review and is periodic by nature. The honest claim is not
   "no ritual" but "the ritual is deferrable without penalty" — concept
   links capture intent at write time, so the backlog never rots into
   an unqueryable pile; sweeping late costs the same as sweeping on
   time.

**Locked contradiction to fix:** the CLI and docs call the flagship
flow the "Synthesis ritual" (`ft review` help text, README) while the
positioning says ft is for people who don't have time for rituals.
The word "ritual" must go. `ft review`'s weekly default shape should be
presented as the *sweep* trigger, not disguised as on-demand.

---

## Session plan

| # | Session                                     | Status  | Depends on |
|---|---------------------------------------------|---------|------------|
| 1 | Story rewrite (README + philosophy)         | done (change: `note-flow-story-rewrite`) | — |
| 2 | Close the loop: cited/processed state       | planned | —          |
| 3 | Ghost promotion                             | planned | —          |
| 4 | Concept drift: detect + merge               | planned | —          |
| 5 | Renames + TUI reshape (tab order)           | planned | 1–4 (last) |

Session 5 is deliberately last: the right names fall out of the
conceptual frame, so settle the frame (and the features that create new
verbs) first.

---

## Session 1 — Story rewrite (README + philosophy.md)

**Goal:** completely user-focused, story-first docs. Lead with the
problem (capture can't wait; filing can't be predicted), not the
feature inventory ("organize, analyze, manipulate notes" + tasks +
time). Tasks/time are adjacent features, not the identity.

**Locked content requirements:**

- The three thesis claims above, told as a story from the user's day
  (conversation about project A reveals project B, no home for the
  thought, etc. — the existing anecdote is good, promote it).
- Fix the ritual contradiction (drop "ritual"; present pull vs. sweep
  honestly).
- Answer the 2026 objection head-on: *why links, when full-text search
  and embeddings retrieve from an unorganized pile?* Answer: a wikilink
  records what a paragraph is **about** at the moment of thought, even
  when the word never appears; co-occurrence and synthesis need
  identity, not similarity; deterministic, local, plain text.
- Tell the alias story. Related-section aliases are the answer to the
  obvious "concept names drift" objection and are currently unsung.
- Tell the completion story: capture-time concept naming is supported
  by autocompletion in the editor — **ft.nvim** for Neovim users,
  Obsidian's own completion for Obsidian users. ft.nvim is currently
  documented nowhere in this repo; that's part of the gap.
- Foreground the actual usage shape: ~90% of ad-hoc capture goes into
  daily-note **sections**; paragraphs are the retrieval unit.
  Paragraph-level granularity (via git blame provenance) is the
  technical differentiator vs. note-level search/backlinks — currently
  underplayed.
- Own the git dependency: git history is the memory; commit cadence is
  the temporal resolution of history/journal dates.

**Candidate framing language** (not locked): "notes earn their
existence" (see session 3); capture → resurface → consolidate as the
three-stage vocabulary for the tool family.

---

## Session 2 — Close the loop: cited/processed state

**Problem (agreed):** nothing is ever "processed." Journal/history
re-show the same paragraphs forever; re-triage cost grows with vault
age. Existing mitigations (synth watermark, dedup of protected sections
on add) are insufficient.

**User's starting input:** when working through a journal to synthesize
a note, that target note should be *part of the working context* — show
clearly what is already in the note and what is missing. This is a
concrete, note-focused form of the "cited-in" badge.

**Assistant's starting hypotheses:**

- **Global citation index and note-context view are not rivals** — the
  index is the substrate, the note-context view is a focused mode of
  it. Build the index once: scan all `ft-synth: true` notes, parse
  `[!ft-source]` callouts, map source spans → citing notes. Callout
  volume is small (hundreds, not millions); count-wise it scales fine
  at scan time.
- **The real scaling problem is paragraph identity, not volume.**
  Callouts pin file + line range + commit; journal entries identify
  paragraphs by current location + blame date. Matching "is this
  paragraph cited?" across subsequent edits/moves is the hard part.
  Candidate anchors: `synth::callout::compute_section_hash` (content
  hash), git blame lineage, tolerated staleness. This is the core
  design question for the session.
- **Processed ≠ cited.** Some paragraphs are noise and will never be
  cited anywhere. If badge-driven triage is the goal, there must be a
  way to mark "seen, irrelevant" (a dismiss action) or the uncited set
  never shrinks. Where does dismiss state live, given "no ft
  database"? (Frontmatter? A vault-local sidecar like the blame cache —
  derivative and deletable? An `ft-reviewed` marker?)
- Surfaces: badge in journal/history feeds (`✓ cited in
  Synthesis/onboarding.md`), `--uncited` filter, and the TUI synthesis
  flow gaining a "target note context" pane.

**Related parking-lot item that naturally joins this session:** the
observe/act grid has an empty cell — time-shaped surfaces (history,
journal feeds) are read-only; consolidation verbs (move-section, synth
grow) should be launchable *from* feed entries. Badges and actions on
feeds are the same surface work.

---

## Session 3 — Ghost promotion

**Problem (agreed):** a ghost link with many mentions and no note is
the vault saying a note has earned its existence. `review` gestures at
this within a window (`?` marker); nothing offers the vault-wide view.
Purest expression of connect-later; candidate tagline: *notes earn
their existence*.

**User's starting input:** the information is already in the graph —
count incoming link edges to ghost nodes. Open question posed: global
view + new command, or leverage the graph view?

**Assistant's starting hypotheses:**

- **Leverage the existing preset pattern, get both surfaces for
  free.** A builtin graph preset (e.g. ghosts ranked by in-degree)
  in `ft_core::graph::preset::builtin()` is automatically picked up by
  CLI `ft graph query --preset …` and the TUI quick-pick — per the
  established "new graph preset" recipe. Check first whether the query
  DSL can already express "kind = Ghost, sort by incoming edge count";
  if sorting-by-degree is missing, that's the core gap to fill.
- **Promotion is an act, not just a view.** The interesting second half
  is the promote action: from a ghost, create the note — plain
  `notes create`, or `synth scaffold` seeded with the ghost's journal
  feed. That connects this session to session 2's surfaces (act from
  feeds) and makes promotion a one-keystroke flow in the graph tab.
- Possible third surface: fold a vault-wide "top unpromoted ghosts"
  line into `review`'s output, since review is the sweep trigger where
  this information changes behavior.

---

## Session 4 — Concept drift: detect + merge

**Correction from user (important):** capture-time completion already
exists — ft.nvim provides concept autocompletion in Neovim; Obsidian
users get Obsidian's. So drift is *reduced* at capture; the remaining
problem is the drift that happens anyway (`[[onboarding]]` vs
`[[onboarding-flow]]` vs `[[new user onboarding]]`).

**User's starting input:** ghost node merging + link updating seems to
be the needed mechanism. Open questions: how to determine what's
important — which links to look at, and how to find their drifted
siblings.

**Assistant's starting hypotheses:**

- **Resolution machinery already exists; detection is the gap.**
  Merging = `notes rename` ghost A → B (vault-wide link rewrite);
  "same concept, keep both names" = Related-section aliases. What's
  missing is the surface that *finds* merge candidates.
- **Candidate generation, two signals multiplied:**
  1. *Name similarity* — shared tokens after normalization/stemming,
     edit distance ("onboarding" ⊂ "onboarding-flow").
  2. *Neighborhood overlap* — the classic synonym signal: true
     duplicates rarely co-occur in the same paragraph (you don't write
     both spellings in one sentence) but share the same co-occurrence
     neighbors. High neighbor overlap + low direct co-occurrence =
     drift suspect. The `related` scoring machinery is adjacent to
     this.
- **Ranking by stakes:** combined mention count. Merging two 2-mention
  ghosts is noise; a 30-mention concept split across three spellings is
  the case that matters. Report ranked by (name similarity ×
  neighborhood overlap × combined weight).
- Surface: a read-only "possible duplicates" report first (CLI), with
  merge/alias as the offered actions. Whether it lives under `review`,
  `related`, or its own verb is a session-5 naming question.

---

## Session 5 — Renames + TUI reshape (last)

**Goal:** make the workflow family legible. Today the five commands of
one idea live in three namespaces (`notes history`, `notes journal`,
`notes related`, top-level `review`, top-level `synth`) and a stranger
cannot see they form capture → resurface → consolidate. TUI tab order
should reflect the same flow.

**Naming critique to act on (candidates, not locked):**

- `journal` → misnamed: a journal is written into; this is a generated
  feed. Philosophy doc already uses the right verb: it "*regathers*
  context." Candidate: `gather`. Large blast radius (CLI, docs, blame
  cache naming, TUI tab) — budget for it.
- `history` vs `journal` → sibling names fail to express the actual
  axis (time-shaped vs topic-shaped); help text currently disambiguates
  by analogy ("the untargeted, time-shaped sibling"). Candidates:
  `recent` / `gather`.
- `review` → generic; collides with PKM "weekly review" and spaced
  repetition; help text conflates it with synthesis ("Synthesis
  ritual"). It's a frequency pulse. Candidate: `pulse`.
- "Synthesis ritual" wording → must go regardless of renames (session 1
  fixes docs; this session fixes CLI help strings).
- Missing verb: one-shot CLI capture — `ft jot "thought with
  [[links]]"` appending to today's daily note. TUI has quick-capture
  presets; the CLI (the scriptable surface) has no capture verb at
  all. Decide whether jot lands here or as its own small change
  earlier.
- New verbs coined by sessions 2–4 (promote, dismiss, a duplicates
  report) get their final names here, in one coherent pass.
- TUI: reorder tabs to follow capture → resurface → consolidate.

---

## Parking lot (captured, unscheduled)

- **The net for what fell through:** all four discovery tools are
  recency- or relevance-shaped. No surface answers "what did I write
  that mentions no concept at all?" or "which ghosts went stale?" If
  the promise is "nothing needs filing," these are the cases the
  promise doesn't cover. (Partially addressed by session 2's dismiss +
  `--uncited`, and session 3's vault-wide ghost view — revisit after
  those land.)
- **Concept-name completion beyond nvim/Obsidian:** a generic
  completion source (e.g. `ft graph concepts --format=completion`) for
  other editors, and documenting ft.nvim in this repo (session 1 covers
  the doc side).
- **Commit-cadence nudge:** journal/history quality degrades with
  sparse commits; consider surfacing "last vault commit was N days ago"
  somewhere, or documenting `ft git sync` cadence as part of the story.
