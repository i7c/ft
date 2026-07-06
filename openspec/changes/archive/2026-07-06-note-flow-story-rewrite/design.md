# Design: note-flow-story-rewrite

## Context

The critique session recorded in
`openspec/explorations/note-flow-reframe.md` locked a sharpened thesis
(local vs. global decisions; retrieval severed from filing; pull vs.
sweep triggers) and found the current docs lead with a feature
inventory and describe the flagship flow as a "Synthesis ritual" —
contradicting the positioning. This change is prose-only: README.md,
docs/guide/philosophy.md, and a wording sweep in docs/guide/index.md
and docs/guide/synthesis.md. Sessions 2–5 (cited-state, ghost
promotion, drift tooling, renames) build on the vocabulary established
here.

## Goals / Non-Goals

**Goals:**

- Problem-first, user-focused story in README and philosophy.
- Establish the prose vocabulary later sessions reuse:
  *capture → resurface → consolidate* for the stages, *pull* and
  *sweep* for the triggers.
- Remove "ritual" from user-facing prose.
- Answer the links-vs-search objection; tell the completion + alias
  drift defenses; foreground daily-note sections, paragraph
  granularity, and the git dependency.

**Non-Goals:**

- No CLI help strings, code identifiers, or comments (session 5).
- No new or renamed commands; docs use today's names (`journal`,
  `review`, `history`, `related`, `synth`). Session 5 owns renames.
- No developer-doc restructuring (docs/architecture.md, CLAUDE.md,
  AGENTS.md keep their section names for now).
- No deletion of currently documented facts — content may move or
  shrink, but accuracy notes (atomic writes, config precedence, etc.)
  survive somewhere in the docs.

## Decisions

**D1 — README structure: story before tables.** New order: (1) a
short problem hook — the un-preplannable day, the conversation that
jumps projects — ending in ft's one-line promise; (2) "how it works"
in four beats: write anywhere (daily note), name concepts with
`[[links]]` (the local decision), resurface later (pull and sweep),
consolidate when a topic earns it; (3) the existing terminal demo
(review → journal → synth scaffold), reframed with pull/sweep labels;
(4) what ft is / is not + Obsidian compatibility table, shortened;
(5) tasks and timeblocks as adjacent features; (6) pointers into the
guide. Rationale: the demo is strong and stays, but a stranger must
meet the problem before the compatibility matrix. Alternative
considered: keep the current identity paragraph and prepend a story —
rejected because the first sentence ("organize, analyze, and
manipulate") is precisely the feature-inventory frame we're replacing.

**D2 — philosophy.md becomes the long-form story, engineering tail
kept but demoted.** New order: the problem; local vs. global
decisions; retrieval severed from filing (synthesis as optional
compression); the two triggers, mapped honestly onto commands; why
links rather than search/embeddings; keeping names honest (ft.nvim /
Obsidian completion, Related-section aliases, `notes rename` as
merge); where notes actually live (daily sections, paragraph
granularity, git as memory, commit cadence, `ft git sync`). The
existing sections "A companion, not a replacement", "The CLI/TUI
split", "Atomic writes", "One way to spell each thing", "What it
deliberately doesn't do", and "Defaults" are kept after the story,
condensed where they repeat it. Rationale: those sections earn trust
with technical readers but are answers to "can I rely on it", which
comes after "why does this exist". Alternative: move the engineering
tail to architecture.md — rejected as scope creep and an audience
mismatch (architecture.md is contributor-facing).

**D3 — replacement vocabulary.** "Synthesis ritual" → the
*consolidate* stage of the flow, entered on demand. Periodic-shaped
commands (`review`, `notes history`) are described as the *sweep*
trigger; topic-shaped commands (`notes journal`, `notes related`) as
the *pull* trigger. The honest claim, stated once and reused: the
sweep is deferrable without penalty, not abolished. These are prose
words, not command names — session 5 decides whether any become verbs.

**D4 — "ritual" sweep scope.** Only README.md and docs/guide/ prose.
The spec's grep scenario (`rg -i ritual README.md docs/guide/` → no
matches) is the acceptance check. Everything else keeps the old word
until session 5 so that docs, help strings, and identifiers rename in
one coherent pass rather than half-renaming now.

**D5 — ft.nvim is mentioned without a link until the URL is
confirmed.** The plugin is documented nowhere in this repo and the
canonical repo/location is unknown at design time. Mention it by name
in the completion story; add the link when the user supplies it (open
question below).

**D6 — examples must be real.** Any command output shown in the
rewritten docs is either verified against a fixture vault or visibly
illustrative (fictional note names are fine; flags, command paths, and
output shape must match the current binary). Rationale: the repo has a
doc-accuracy review culture (docs/doc-accuracy-review.md) and the
README demo is the most-copied text in the project.

## Risks / Trade-offs

- [Marketing-speak drift] The story framing could slide into
  landing-page tone that clashes with the repo's dry voice. →
  Mitigation: keep the existing docs' register — concrete nouns,
  short declaratives, no superlatives; reuse the conversation anecdote
  already in philosophy.md rather than inventing personas.
- [Docs claim what session 2–4 features don't yet do] The thesis
  mentions triage/promotion ideas that aren't shipped. → Mitigation:
  the rewrite describes only shipped behavior; the forward-looking
  frame stays in the exploration doc.
- [Half-renamed world] Users will read "sweep"/"pull" prose next to a
  command literally named `review` with "Synthesis ritual" in its
  `--help`. → Accepted for one session; session 5 closes the gap.
  The docs never quote the old help text.
- [Link rot] Restructuring moves anchors other docs may point at. →
  Mitigation: final pass resolves every relative link in the four
  touched files and greps the repo for inbound links to renamed
  headings.

## Open Questions

- Canonical ft.nvim repository URL (and exact plugin name/casing) —
  needed before the completion story can link out; mention-only until
  then.
- Does docs/guide/index.md order the guide with philosophy first? If
  not, decide whether reordering the index belongs in this change
  (cheap) or is left alone.
