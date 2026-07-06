# notes-history — delta

## ADDED Requirements

### Requirement: Cited badge in history text output
`ft notes history` SHALL annotate entries with the same badge grammar
as `ft notes journal`: `cited: <note stem>` / `cited*: <note stem>`,
first citing note plus `+N` overflow, uncited entries unchanged.

#### Scenario: History entry shows badge
- **WHEN** a paragraph edited in the window is pinned in a synth note
- **THEN** its history entry renders the `cited:` badge line

### Requirement: cited_in in history JSON
`ft notes history --json` entries SHALL gain the same additive
`cited_in` array of `{note, stale}` objects as the journal.

#### Scenario: JSON parity with journal
- **WHEN** the same paragraph appears in both journal and history JSON
- **THEN** both report identical `cited_in` contents

### Requirement: --uncited filter on history
`ft notes history --uncited` SHALL keep only entries whose state is
not `Cited` (stale kept), composing with the existing window flags.

#### Scenario: Incremental sweep
- **WHEN** the user runs `ft notes history --since 7d --uncited`
- **THEN** only paragraphs from the window not yet pinned
  byte-identically in any synth note are listed
