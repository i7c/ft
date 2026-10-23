## MODIFIED Requirements

### Requirement: Core ghost ranking
`ft_core::graph::ghosts::rank_ghosts(graph)` SHALL return every ghost
node with its mention count, where the count is the number of
**distinct paragraph nodes** holding a `ParagraphLink` edge to the
ghost (multiple mentions inside one paragraph count once). Results SHALL be
sorted by count descending, then ghost name ascending. The ranking SHALL
require no git history.

#### Scenario: Distinct-paragraph dedup
- **WHEN** one paragraph mentions `[[foo]]` three times and another
  paragraph mentions it once
- **THEN** `rank_ghosts` reports `foo` with a count of 2

#### Scenario: Deterministic tie-break
- **WHEN** two ghosts have equal counts
- **THEN** they order alphabetically by name

### Requirement: ft notes ghosts CLI
`ft notes ghosts` SHALL print the ranked ghost list as `(N) [[ghost]]`
rows, highest count first. It
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
