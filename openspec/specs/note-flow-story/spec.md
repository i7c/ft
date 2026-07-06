# note-flow-story Specification

## Purpose
The user-facing narrative contract: what README.md and the guide docs
must communicate about ft's note-taking thesis, which objections they
must answer, and which framings they must avoid. Established by the
2026-07-05 critique session (openspec/explorations/note-flow-reframe.md).

## Requirements
### Requirement: README leads with the problem, not the feature list
README.md SHALL open with the problem ft solves — capture can't wait
and filing can't be predicted — told as a user story, before any
enumeration of features or compatibility tables. Task management and
time management SHALL be presented as adjacent features layered on the
note-flow core, not as the tool's identity.

#### Scenario: A stranger reads the first screen of the README
- **WHEN** a reader who has never seen ft reads the README from the top
  through the first section break
- **THEN** they can state the problem ft solves (instant, unfiled
  capture that stays retrievable) without having encountered a feature
  inventory, a compatibility table, or the words "tasks" and
  "timeblocks" as headline items

### Requirement: The three thesis claims appear in README and philosophy
Both README.md (concise form) and docs/guide/philosophy.md (long form)
SHALL state the three locked thesis claims: (1) "add anywhere" removes
the global decision ("where does this go?") and keeps only the local
one ("what is this about?" — a `[[concept]]` mention); (2) because
journal/related make the unsorted pile retrievable by concept, filing
becomes optional, not accelerated — synthesis is optional compression,
not owed maintenance; (3) there are two legitimate triggers, pull
("everything on X, now") and sweep ("what accumulated?"), and the
sweep is deferrable without penalty because links capture intent at
write time.

#### Scenario: Reader can distinguish ft from a filing-system tool
- **WHEN** a reader finishes the philosophy section of either document
- **THEN** they can explain that ft shifts organization from a global
  location decision to a local naming decision, and that retrieval does
  not depend on filing ever happening

#### Scenario: Pull and sweep are presented honestly
- **WHEN** the docs describe `ft notes pulse` / `ft notes recent`
  (window-shaped commands) alongside `ft notes gather` / `ft notes
  related` (topic-shaped commands)
- **THEN** the window-shaped commands are framed as the sweep trigger
  and the topic-shaped commands as the pull trigger, and no text claims
  ft eliminates periodic sweeping — only that deferring it carries no
  penalty

### Requirement: No "flow" framing in user-facing docs
README.md and all prose under docs/guide/ SHALL NOT describe any ft
workflow as a "flow". CLI help strings, code identifiers, developer
docs (docs/architecture.md, CLAUDE.md, AGENTS.md), openspec/specs, and
archived changes are out of scope for this requirement (deferred to the
renames session).

#### Scenario: Grep for flow in user-facing docs
- **WHEN** `rg -i flow README.md docs/guide/` is run at the repo root
- **THEN** it returns no matches

### Requirement: Philosophy answers the links-vs-search objection
docs/guide/philosophy.md SHALL explicitly pose and answer the
objection "why maintain wikilinks when full-text search or embeddings
can retrieve from an unorganized pile?" The answer SHALL cover: a
wikilink records what a paragraph is about at the moment of thought,
even when the concept's name never appears in the text; co-occurrence
scoring and synthesis need stable identity, not similarity; and the
mechanism is deterministic, local, and plain text.

#### Scenario: The objection is visible, not implicit
- **WHEN** a skeptical reader scans philosophy.md section headings and
  opening sentences
- **THEN** they find the search/embeddings alternative named and
  addressed, not merely an unargued assertion that links are better

### Requirement: The drift defenses are told as part of the story
docs/guide/philosophy.md SHALL present the two defenses against
concept-name drift as part of the core story: capture-time completion
(the ft.nvim plugin for Neovim users; Obsidian's built-in wikilink
completion for Obsidian users) and after-the-fact reconciliation
(Related-section aliases feeding journal/related; `ft notes rename` to
merge a drifted name vault-wide).

#### Scenario: A Neovim user learns completion exists
- **WHEN** a reader who captures in $EDITOR reads the philosophy doc
- **THEN** they learn ft.nvim provides concept autocompletion at
  capture time, the first mention of ft.nvim anywhere in this repo's
  docs

#### Scenario: The alias mechanism is presented as the drift answer
- **WHEN** the docs raise the "names drift" concern
- **THEN** Related-section aliases are presented as the designed
  answer, with rename as the merge tool — not left as an unsung
  implementation detail

### Requirement: The capture surface and granularity story is explicit
The docs SHALL state where capture actually happens and at what grain
retrieval operates: ad-hoc capture goes into daily-note sections (the
default capture surface); paragraphs are the retrieval unit; and
paragraph-level provenance via git blame is what distinguishes ft's
resurfacing from note-level search and backlinks. The docs SHALL also
own the git dependency: git history is the memory, and commit cadence
sets the temporal resolution of history/journal dates.

#### Scenario: Reader understands why commits matter
- **WHEN** a reader sets up ft without a git habit
- **THEN** the philosophy or guide text has told them that sparse
  commits mean coarse journal/history dates, and pointed at `ft git
  sync` as the cadence tool

### Requirement: Cross-document links remain intact
The rewritten documents SHALL keep existing cross-references resolving:
README links into docs/guide/philosophy.md, philosophy.md links into
synthesis.md and neighboring guide pages, and docs/guide/index.md
continues to index the guide accurately.

#### Scenario: No dangling links after rewrite
- **WHEN** every relative link in README.md, docs/guide/philosophy.md,
  docs/guide/index.md, and docs/guide/synthesis.md is resolved against
  the repo tree
- **THEN** every target exists
