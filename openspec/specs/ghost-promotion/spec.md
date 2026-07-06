# ghost-promotion Specification

## Purpose
Vault-wide ghost ranking and the promote actions: which concepts
have earned their page, and one keystroke to give it to them.
Created by archiving change ghost-promotion.

## Requirements
### Requirement: Core ghost ranking
`ft_core::graph::ghosts::rank_ghosts(graph)` SHALL return every ghost
node with its mention count, where the count is the number of
**distinct paragraph nodes** holding a `ParagraphLink` edge to the
ghost (multiple mentions inside one paragraph count once — the same
dedup rule as `ft notes pulse`). Results SHALL be sorted by count
descending, then ghost name ascending. The ranking SHALL require no
git history.

#### Scenario: Distinct-paragraph dedup
- **WHEN** one paragraph mentions `[[foo]]` three times and another
  paragraph mentions it once
- **THEN** `rank_ghosts` reports `foo` with a count of 2

#### Scenario: Deterministic tie-break
- **WHEN** two ghosts have equal counts
- **THEN** they order alphabetically by name

### Requirement: ft notes ghosts CLI
`ft notes ghosts` SHALL print the ranked ghost list as `(N) [[ghost]]`
rows (the same row grammar as `ft notes pulse`), highest count first. It
SHALL support `--limit <n>` (truncate), `--min-mentions <n>` (drop
ghosts below the threshold; default 1), and `--json` (array of
`{target, mentions}` objects). An empty result SHALL print
`no ghosts in the vault` and exit 0.

#### Scenario: Ranked table output
- **WHEN** a vault has ghosts with 3, 1, and 5 mentions
- **THEN** `ft notes ghosts` lists them 5-3-1, each as `(N) [[name]]`

#### Scenario: Filters compose
- **WHEN** `--min-mentions 2 --limit 1` is passed on that vault
- **THEN** exactly one row is printed: the 5-mention ghost

#### Scenario: JSON shape
- **WHEN** `--json` is passed
- **THEN** stdout is a JSON array whose entries have `target` (string)
  and `mentions` (integer), in ranked order

### Requirement: Ranked ghost ordering in graph walks
Wherever the graph walk's deterministic ordering places ghost nodes
(root selection and sibling sort), ghosts SHALL order by mention count
descending, then name ascending — so `ft graph query --preset ghosts`
and the TUI graph tab present the ranked view without new query
syntax. Ordering of non-ghost kinds SHALL be unchanged.

#### Scenario: Preset becomes the ranked view
- **WHEN** `ft graph query --preset ghosts` runs on a vault whose
  ghosts have counts 1, 4, and 2
- **THEN** the output lists the ghosts in 4-2-1 order

### Requirement: Mention counts on TUI ghost rows
Ghost rows in the graph tab SHALL render the mention count after the
label (e.g. `G activation (3)`), using the same core ranking, computed
per snapshot generation rather than per frame.

#### Scenario: Count visible in the tree
- **WHEN** the graph tab shows a ghost mentioned in three distinct
  paragraphs
- **THEN** its row carries `(3)`

### Requirement: Scaffold-seeded promotion
A `graph.promote-ghost` command SHALL be available on ghost rows in
the graph tab. It SHALL create the note at the ghost's path as a synth
note scaffolded with every paragraph mentioning the ghost (via the
journal feed and `plan/apply_synth_scaffold`), set
`ft-synth-targets: ["[[<ghost>]]"]`, request a graph refresh, and open
the editor at the new note. When the vault lacks git history or the
scaffold planner refuses (dirty sources), the command SHALL surface
the error as a toast and change nothing. Existing blank/template
creates on ghost rows SHALL be unchanged.

#### Scenario: One-keystroke promotion with material
- **WHEN** the user invokes `graph.promote-ghost` on a ghost with four
  mentioning paragraphs in a git-backed vault
- **THEN** a synth note is created at the ghost's path containing four
  `[!ft-source]` sections and the `ft-synth-targets` frontmatter, and
  the ghost row becomes a note row after the refresh

#### Scenario: Non-ghost row is a no-op
- **WHEN** the command is invoked with a note or directory row selected
- **THEN** nothing is created and a toast explains the command applies
  to ghost rows

### Requirement: Registry and docs integration
The new command SHALL be declared in the graph tab's command/keymap
statics (default chord chosen to pass `ft commands check-keymap`),
appear in the `?` overlay and `ft commands list`, and
`docs/keybindings.md` SHALL be regenerated.

#### Scenario: Docs stay in sync
- **WHEN** `cargo run --release -q -- commands docs --check` runs after
  the change
- **THEN** it passes
