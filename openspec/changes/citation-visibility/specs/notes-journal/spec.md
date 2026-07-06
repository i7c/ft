# notes-journal — delta

## ADDED Requirements

### Requirement: Cited badge in journal text output
`ft notes journal` SHALL annotate each entry whose citation state is
not `Uncited` with a badge line: `cited: <note stem>` for exact
citations, `cited*: <note stem>` for stale ones; when multiple synth
notes cite the entry, the first (path-sorted) is shown followed by
`+N`. Uncited entries render unchanged.

#### Scenario: Cited entry shows badge
- **WHEN** a journal entry's paragraph is pinned byte-identically in
  `Synthesis/foo.md`
- **THEN** the entry renders a `cited: foo` badge line

#### Scenario: Stale entry shows starred badge
- **WHEN** the paragraph was edited after being pinned
- **THEN** the entry renders `cited*: foo`

### Requirement: cited_in in journal JSON
`ft notes journal --json` entries SHALL gain a `cited_in` array of
`{note, stale}` objects (vault-relative note path, boolean), empty
when uncited. Existing fields SHALL be unchanged.

#### Scenario: JSON carries citation state
- **WHEN** `--json` is used on a feed with one cited and one uncited
  entry
- **THEN** the cited entry has a non-empty `cited_in` and the uncited
  entry has `cited_in: []`

### Requirement: --uncited filter on journal
`ft notes journal --uncited` SHALL keep only entries whose state is
not `Cited` (stale entries are kept). It SHALL compose with existing
flags (`--link`, `--since`/`--range`, `--in-window`, `--json`).

#### Scenario: Filter drops exact citations only
- **WHEN** a feed contains a cited, a stale, and an uncited entry and
  `--uncited` is passed
- **THEN** the stale and uncited entries remain and the cited entry is
  dropped
