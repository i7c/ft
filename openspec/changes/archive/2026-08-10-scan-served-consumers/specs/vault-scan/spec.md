# vault-scan

## ADDED Requirements

### Requirement: File metadata captured at scan time
Each `ParsedFile` SHALL carry the file's modification time (`mtime`) and its line count (`line_count`), captured during the scan's single walk and single read. The walker SHALL obtain the mtime from the same walk pass (via the walker's metadata support, not a second filesystem traversal), and `parse_file` SHALL compute the line count from the content it already read. `scan::markdown_files_with_mtime` SHALL NOT exist as a separate mtime source; `Scan::files` SHALL be the only mtime source.

#### Scenario: mtime from the same walk
- **WHEN** a vault is scanned
- **THEN** every `ParsedFile::mtime` equals the file's on-disk modification time as captured during the scan's walk, and no second walk or per-file metadata pass is performed

#### Scenario: line count matches content
- **WHEN** a scanned file contains N lines
- **THEN** `ParsedFile::line_count == N`

### Requirement: Vault-reading consumers served from the scan
Consumers that derive from parse artifacts (frontmatter markers, headings, mtimes) SHALL be served from the scan's `ParsedFile` data when a scan is available, instead of re-reading the file: (a) the history feed's synth-note exclusion and the link-review's synth-callout-exclusion gate SHALL use the scan's captured frontmatter; (b) the journal's Related-alias resolution SHALL use the scan's headings and line count; (c) the TUI file/heading search picker SHALL use the scan's paths and headings for per-keystroke filtering; (d) mtime consumers SHALL use the scan's captured mtimes. When a file is absent from the scan (scan read error, or file created after the scan), the consumer SHALL fall back to reading the file directly. In the TUI, where the scan is a snapshot, these consumers SHALL be eventually consistent: file-state changes made after the scan are not visible until the next graph rebuild. CLI consumers SHALL keep fresh semantics via a per-invocation scan.

#### Scenario: Marker gate avoids disk reads
- **WHEN** `build_recent` / `compute_pulse` run over a scan whose files carry captured frontmatter
- **THEN** files the scan marks as non-synth are not read from disk for the marker check, and the history/link-review output matches the marker state captured by the scan

#### Scenario: Fallback when file absent from the scan
- **WHEN** a file touched by the git window or targeted by the journal has no `ParsedFile` in the scan (e.g. created after the scan)
- **THEN** the consumer reads the file directly and behaves exactly as before this change

#### Scenario: TUI eventual consistency, CLI freshness
- **WHEN** a note becomes a synth note after the snapshot was built
- **THEN** in the TUI the history feed and link review continue to treat it as non-synth until the next graph rebuild, and in the CLI (`ft notes recent` / `ft notes pulse`) the fresh per-invocation scan reflects the marker immediately

#### Scenario: Picker filters in memory
- **WHEN** the TUI file/heading picker is open with a snapshot installed
- **THEN** each keystroke's filtering uses in-memory scan paths and headings (no vault walk, no file reads); before the first snapshot lands, the picker falls back to the vault-based search

#### Scenario: CLI find unchanged
- **WHEN** `ft find` is run
- **THEN** it continues to perform a filesystem walk with no scan build and no parse of vault files
