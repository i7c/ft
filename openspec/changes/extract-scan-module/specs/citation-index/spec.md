# citation-index

## MODIFIED Requirements

### Requirement: CitationIndex build
`ft_core::synth::citations::CitationIndex` SHALL be built from a vault scan: `CitationIndex::build(root, scan)` SHALL discover synth notes from the scan's captured frontmatter (`ParsedFile::frontmatter` with `ft.synth.enabled: true`) — no re-walk of the vault and no reads of non-synth files — and SHALL read each synth-marked note's content exactly once to parse its `[!ft-source]` callouts via `synth::callout::parse`. Callouts SHALL be indexed by `(source_path, content_hash_prefix)` for exact lookup plus a per-path line-interval list for stale lookup. Notes that fail to parse SHALL be skipped with a diagnostic, not abort the build.

#### Scenario: Index covers all synth notes
- **WHEN** a vault contains two synth notes citing paragraphs from three source files
- **THEN** `CitationIndex::build` indexes every callout from both notes, keyed by source path and content hash

#### Scenario: Malformed synth note does not abort
- **WHEN** one synth note contains an unparseable callout header
- **THEN** the index builds from the remaining callouts and reports the skipped note as a diagnostic

#### Scenario: No second read pass
- **WHEN** the index is built over a vault where exactly one of one hundred files is a synth note
- **THEN** synth-note discovery reads no file contents (it uses the scan's frontmatter capture), and exactly the one synth note's content is read from disk for callout parsing

#### Scenario: Index built from the same scan as the graph
- **WHEN** a `Scan` is scanned once and used for both `Graph::build` and `CitationIndex::build`
- **THEN** both derive from the same read pass, so task line numbers, graph nodes, and citation targets all agree
