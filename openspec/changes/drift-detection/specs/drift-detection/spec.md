# drift-detection

Finding drifted concept names — one idea split across several
`[[spellings]]` — and pointing at the existing resolution machinery.

## ADDED Requirements

### Requirement: Drift candidate detection
`ft_core::graph::drift::detect_drift(graph, vault)` SHALL return
ranked candidate pairs of concepts (notes and ghosts with at least one
distinct-paragraph mention). A pair SHALL only be reported when its
name similarity — computed over normalized tokens (lowercased,
`.md`/directory stripped, split on whitespace/`-`/`_`/`/`, trailing-`s`
trimmed) with token containment, token overlap, and edit distance —
passes the gate. Each reported pair SHALL carry its name similarity,
neighborhood overlap (shared co-occurrence profile via the `related`
scoring machinery, each side excluded from the other's profile),
direct co-occurrence count (paragraphs mentioning both), combined
mention weight, and final score. No file SHALL be read or written
beyond the graph/vault inputs; detection is read-only.

#### Scenario: Compound-name drift is detected
- **WHEN** a vault mentions `[[onboarding]]` and `[[onboarding-flow]]`
  in separate paragraphs that share co-occurring concepts
- **THEN** the pair is reported

#### Scenario: Dissimilar names never pair
- **WHEN** two concepts share neighbors but have no name-token overlap
  (e.g. `[[onboarding]]` and `[[activation]]`)
- **THEN** no pair is reported for them

### Requirement: Ordering properties
Ranking SHALL satisfy: (1) a pair with higher name similarity and
higher neighborhood overlap ranks above a pair that is lower on both,
other signals equal; (2) direct co-occurrence lowers a pair's rank —
two similarly-named concepts frequently mentioned in the same
paragraph rank below an otherwise-equal pair that never co-occurs;
(3) higher combined mention weight raises rank, so high-stakes splits
surface first.

#### Scenario: Co-occurring near-names rank below true drift
- **WHEN** pair A (never co-occurring, shared neighbors) and pair B
  (same name similarity and weight, frequently co-occurring in the
  same paragraphs) are both detected
- **THEN** pair A ranks above pair B

#### Scenario: Stakes raise rank
- **WHEN** two pairs have equal signals but one concerns concepts with
  ten times the combined mentions
- **THEN** the heavier pair ranks first

### Requirement: Resolution suggestions
Each reported pair SHALL include a textual resolution suggestion:
when at least one side is a ghost, a merge command folding the
lower-mention side into the keeper (`ft notes rename "[[lesser]]"
"<keeper>"`), where a real note is always the keeper over a ghost;
when both sides are real notes, an alias suggestion (list the lesser
under the keeper's `## Related` heading) noting that content merging
is manual. The tool SHALL NOT execute any suggestion.

#### Scenario: Ghost folds into the note
- **WHEN** ghost `[[onboarding-flow]]` (4 mentions) pairs with note
  `onboarding.md` (31 mentions)
- **THEN** the suggestion is `ft notes rename "[[onboarding-flow]]"
  "onboarding"` and nothing is written

#### Scenario: Note pair gets alias advice
- **WHEN** two real notes pair
- **THEN** the suggestion names the Related-section alias mechanism
  and no rename command is offered

### Requirement: ft notes drift CLI
`ft notes drift` SHALL print the ranked pairs as
`[[keeper]] (N) ↔ [[lesser]] (M)` header lines (ghost sides marked
with the `?` suffix, counts = distinct paragraphs) each followed by an
indented suggestion line. It SHALL support `--limit <n>`, `--json`
(array of pair objects carrying both sides' `{target, is_ghost,
mentions}`, the signal values, `score`, and `suggestion`), and
`--no-color`. An empty result SHALL print `no drift candidates found`
and exit 0.

#### Scenario: Report row shape
- **WHEN** the onboarding drift pair is the top candidate
- **THEN** output contains `[[onboarding]] (31) ↔ [[onboarding-flow]]? (4)`
  followed by an indented `merge:` line

#### Scenario: JSON is scriptable
- **WHEN** `--json` is passed
- **THEN** stdout is a JSON array whose entries expose both targets,
  ghost flags, mention counts, `name_similarity`,
  `neighborhood_overlap`, `direct_cooccurrence`, `score`, and
  `suggestion`

#### Scenario: Clean vault exits zero
- **WHEN** no pair passes the gate
- **THEN** `no drift candidates found` is printed and the exit code
  is 0
