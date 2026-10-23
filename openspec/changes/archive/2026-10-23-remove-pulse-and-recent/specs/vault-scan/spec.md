## MODIFIED Requirements

### Requirement: Vault-reading consumers served from the scan
Consumers that derive from parse artifacts (frontmatter markers, headings, mtimes) SHALL be served from the scan's `ParsedFile` data when a scan is available, instead of re-reading the file: (a) the paragraph-search index's synth-note exclusion SHALL use the scan's captured frontmatter; (b) the TUI file/heading search picker SHALL use the scan's paths and headings for per-keystroke filtering; (c) mtime consumers SHALL use the scan's captured mtimes. When a file is absent from the scan (scan read error, or file created after the scan), the consumer SHALL fall back to reading the file directly. In the TUI, where the scan is a snapshot, these consumers SHALL be eventually consistent: file-state changes made after the scan are not visible until the next graph rebuild. CLI consumers SHALL keep fresh semantics via a per-invocation scan.

#### Scenario: Marker gate avoids disk reads
- **WHEN** the search index is built over a scan whose files carry captured frontmatter
- **THEN** files the scan marks as non-synth are not read from disk for the marker check, and the search results match the marker state captured by the scan

#### Scenario: Fallback when file absent from the scan
- **WHEN** a file targeted for a heading or marker read has no `ParsedFile` in the scan (e.g. created after the scan)
- **THEN** the consumer reads the file directly and behaves exactly as before the scan-served change

#### Scenario: TUI eventual consistency, CLI freshness
- **WHEN** a note becomes a synth note after the snapshot was built
- **THEN** in the TUI the search index continues to treat it as non-synth until the next graph rebuild, and in the CLI (`ft notes search`) the fresh per-invocation scan reflects the marker immediately

#### Scenario: Picker filters in memory
- **WHEN** the TUI file/heading picker is open with a snapshot installed
- **THEN** each keystroke's filtering uses in-memory scan paths and headings (no vault walk, no file reads); before the first snapshot lands, the picker falls back to the vault-based search

#### Scenario: CLI find unchanged
- **WHEN** `ft find` is run
- **THEN** it walks the vault directly, with no scan dependency
