# search-relevance-eval Specification

## Purpose
A relevance evaluation harness for paragraph search: graded `query →
paragraph` judgments, a committed fixture vault, nDCG@10 / MRR / recall@10
metrics with a ratchet in `cargo test`, and an opt-in report against a real
vault. Created by archiving change search-relevance-overhaul.
## Requirements
### Requirement: Judgment file format

Relevance judgments SHALL live in a TOML file of `[[case]]` tables. Each case
SHALL have a `query` string, an optional `any` boolean (default `false`), an
optional `note` string describing what the case probes, and one or more
`[[case.relevant]]` entries with a vault-relative `path`, a 1-indexed `line`,
and an integer `grade` from 1 (somewhat relevant) to 3 (ideal). A judged entry
SHALL match a result whose path is equal and whose line range contains `line`,
so judgments survive small paragraph-boundary changes. Loading SHALL fail with a
message naming the case index when a field is missing or a grade is out of
range.

#### Scenario: Range-contained judgment matches

- **WHEN** a case judges `notes/a.md` line 12 and a result covers `notes/a.md` L10-14
- **THEN** that result counts as the judged paragraph

#### Scenario: Invalid grade

- **WHEN** a case has `grade = 5`
- **THEN** loading fails with an error naming the case index and the allowed range 1–3

### Requirement: Committed evaluation fixture

The repository SHALL ship an evaluation vault under
`tests/fixtures/search-eval/` and a judgment file next to it covering at least
25 cases. The cases SHALL include at least one probe of each of: length
normalization (focused paragraph vs long list), rare vs common terms, a
context-field match (title, alias, heading, tag), accented text in English and
Portuguese, stem variants, the `path:`, `tag:`, and `scope:` filters, `near:`
proximity, and a pre-existing DSL feature (phrase, link, fuzzy, exclude) that
must not regress. Date filters are excluded from the fixture evaluation (blame
dates of committed fixture files depend on commit time) and SHALL be covered by
unit tests against a temporary git repository instead.

#### Scenario: Coverage of probe classes

- **WHEN** the judgment file is loaded
- **THEN** it contains at least 25 cases and every probe class listed above is represented by at least one case's `note`

### Requirement: Metrics and ratchet test

A `cargo test` integration test SHALL build the index from the evaluation vault
(with its graph, and with `FT_TODAY` pinned), run every case, and compute per
case and on average: nDCG@10 with gain `2^grade − 1`, MRR (reciprocal rank of
the first judged result), and recall@10. The test SHALL fail when the mean
nDCG@10 or mean MRR falls below the thresholds recorded in a committed
`baseline.toml` beside the judgments, and SHALL print a per-case table
(query, nDCG@10, MRR, recall@10) when run with `--nocapture`. The first
recorded baseline SHALL be the pre-change engine's scores, and the thresholds
SHALL be raised to the achieved scores minus 0.01 when the change completes.

#### Scenario: Regression fails the build

- **WHEN** a ranking change lowers the mean nDCG@10 below the baseline threshold
- **THEN** `cargo test --workspace` fails, naming the metric, the threshold, and the achieved value

#### Scenario: Per-case report

- **WHEN** the eval test runs with `--nocapture`
- **THEN** stdout lists every case's query with its nDCG@10, MRR, and recall@10, followed by the means

### Requirement: Real-vault evaluation run

The harness SHALL run against the vault named by `FT_VAULT` and print the
per-case report without asserting thresholds when `FT_REAL_VAULT_TESTS=1` and
`FT_SEARCH_EVAL_JUDGMENTS` names a judgment file. The run SHALL never be
on by default, and SHALL be skipped (not failed) when either variable is unset.

#### Scenario: Gated real-vault run

- **WHEN** `FT_REAL_VAULT_TESTS=1`, `FT_VAULT` and `FT_SEARCH_EVAL_JUDGMENTS` are set and the test runs
- **THEN** the per-case report for that vault is printed and the test passes regardless of scores

#### Scenario: Skipped by default

- **WHEN** `cargo test --workspace` runs without those variables
- **THEN** the real-vault eval test is skipped

