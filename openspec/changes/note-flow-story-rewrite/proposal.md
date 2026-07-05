# Proposal: note-flow-story-rewrite

## Why

ft's README and philosophy docs lead with a feature inventory
("organize, analyze, and manipulate notes" plus tasks and time) instead
of the problem ft actually exists to solve: capture can't wait and
filing can't be predicted. Worse, the docs describe the flagship flow
as a "Synthesis ritual" — directly contradicting the positioning that
ft is for people who don't have time for rituals. The 2026-07-05
critique session (`openspec/explorations/note-flow-reframe.md`,
session 1) locked the sharpened thesis and the content requirements;
this change writes that story into the user-facing docs.

## What Changes

- **README.md rewrite**: lead with the problem story, then the thesis
  (local vs. global decisions at capture; retrieval severed from
  filing; pull vs. sweep triggers), then the demo. Tasks/time presented
  as adjacent features, not the identity. Drop "ritual" wording.
- **docs/guide/philosophy.md rewrite**: the long-form, user-focused
  version of the same story, replacing the technically-framed sections.
  Must answer the links-vs-search objection, tell the alias story
  (Related-section aliases as the answer to concept-name drift), tell
  the completion story (ft.nvim for Neovim, Obsidian's own completion
  for Obsidian users), foreground daily-note sections as the capture
  surface and paragraphs as the retrieval unit, and own the git
  dependency (commit cadence = temporal resolution of history/journal).
- **"ritual" prose sweep in user-facing guide docs**
  (docs/guide/index.md, docs/guide/synthesis.md): replace the framing,
  keep the content.
- **Out of scope** (deferred to session 5, renames): CLI help strings
  (`ft/src/main.rs`, `ft/src/cmd/review.rs`), code identifiers and
  comments, developer-doc section names (docs/architecture.md,
  CLAUDE.md, AGENTS.md), openspec/specs wording, historical archives.

## Capabilities

### New Capabilities

- `note-flow-story`: the user-facing narrative contract — what the
  README and guide docs must communicate about ft's note-taking
  thesis (problem-first framing, the three locked claims, the
  objections they must answer, and the wording they must avoid).

### Modified Capabilities

<!-- none — this change alters documentation content only; no runtime
behavior or existing spec-level requirements change -->

## Impact

- Files: `README.md`, `docs/guide/philosophy.md`, `docs/guide/index.md`,
  `docs/guide/synthesis.md`.
- No code, CLI surface, or test changes. `cargo run -q -- commands docs
  --check` unaffected (no keymap/registry changes).
- Cross-doc links must stay intact (philosophy.md ↔ synthesis.md,
  README → docs/guide/philosophy.md).
- Sessions 2–5 of the exploration plan build on the vocabulary this
  change establishes (capture → resurface → consolidate).
