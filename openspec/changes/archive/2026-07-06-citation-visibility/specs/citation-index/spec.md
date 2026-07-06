# citation-index

## ADDED Requirements

### Requirement: CitationIndex build
`ft_core::synth::citations::CitationIndex` SHALL be buildable from a
vault scan by iterating notes whose frontmatter carries
`ft-synth: true`, parsing their `[!ft-source]` callouts via
`synth::callout::parse`, and indexing every callout by
`(source_path, content_hash_prefix)` for exact lookup plus a per-path
line-interval list for stale lookup. Notes that fail to parse SHALL be
skipped with a diagnostic, not abort the build.

#### Scenario: Index covers all synth notes
- **WHEN** a vault contains two synth notes citing paragraphs from
  three source files
- **THEN** `CitationIndex::build` indexes every callout from both
  notes, keyed by source path and content hash

#### Scenario: Malformed synth note does not abort
- **WHEN** one synth note contains an unparseable callout header
- **THEN** the index builds from the remaining callouts and reports
  the skipped note as a diagnostic

### Requirement: Three-state lookup
`CitationIndex::lookup(source_path, line_range, body)` SHALL return
`Cited { notes }` when at least one callout pins the same source path
with a byte-identical body (hash-prefix fast reject, exact body
compare to confirm — the same rule as `accrete::filter_missing`);
otherwise `CitedStale { notes }` when at least one callout pins the
same source path with a line range overlapping `line_range` and a
different body; otherwise `Uncited`. Lookup results SHALL name the
citing synth notes by vault-relative path.

#### Scenario: Exact match is Cited
- **WHEN** a paragraph's current text equals a callout body pinned
  from the same source file
- **THEN** lookup returns `Cited` naming the callout's synth note

#### Scenario: Edited-since-cited is CitedStale
- **WHEN** a callout pins lines 10–12 of a source file and the
  paragraph now spanning lines 10–13 of that file has different text
- **THEN** lookup returns `CitedStale` naming that synth note

#### Scenario: Unrelated paragraph is Uncited
- **WHEN** no callout pins the paragraph's source file, or callouts
  pin non-overlapping ranges with different bodies
- **THEN** lookup returns `Uncited`

### Requirement: Consistency with scaffold dedup
For any journal entry, `lookup` against a single note SHALL classify
the entry `Cited` if and only if `accrete::filter_missing` would drop
it when appending to that note, so badges never disagree with
plan-time dedup.

#### Scenario: Badge agrees with plan
- **WHEN** an entry shows `in note` for target note N in note-context
  mode and the user scaffolds/grows N with that entry in the feed
- **THEN** the plan reports it as dedup-skipped

### Requirement: Build points
The CLI SHALL build the index per invocation inside the commands that
render feeds. The TUI SHALL build it in the background graph-rebuild
worker and expose it to tabs together with the shared graph snapshot
(same generation), so tabs never scan or parse synth notes themselves.

#### Scenario: TUI tabs read the snapshot index
- **WHEN** a mutation triggers `request_graph_refresh` and the rebuild
  completes
- **THEN** Journal/History rows re-badge from the new index without
  any tab-local vault scan
