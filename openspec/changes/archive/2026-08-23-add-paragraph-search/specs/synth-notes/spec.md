# synth-notes

## MODIFIED Requirements

### Requirement: ft notes synth scaffold command

`ft notes synth <target.md> --search "<query>" [--any] [--sort relevance|date] [--link "[[Foo]]" ...] [--from <path>:<line> ...] [--no-edit]` SHALL generate or append protected-section scaffolding into the target note. `--search` SHALL be the primary sourcing flag and is repeatable with `--any`/`--sort` (see the paragraph-search capability); `--link` and `--from` SHALL remain functional as a transitional path (`--link` is deprecated: it lowers to an any-mode search over the given links and performs no Related-alias resolution). At least one of `--search`, `--link`, or `--from` SHALL be required. The window flags `--in-window`, `--since`, and `--range` SHALL NOT exist. When the target file does not exist, the command SHALL create it with `ft.synth.enabled: true` frontmatter, followed by the scaffolded sections as the body. When the target exists, the command SHALL append (at end of file) the new sections separated from existing content by one blank line; the append path SHALL drop any entry whose `(source_path, body)` is already pinned in the note (dedup-on-append invariant), so re-running scaffold with the same sourcing flags is idempotent. After writing, the command SHALL open `$EDITOR` at the bottom of the file unless `--no-edit` is passed.

#### Scenario: Create new synth note from search

- **WHEN** `ft notes synth Synthesis/topic.md --search "eigen memoization"` is run and `Synthesis/topic.md` does not exist
- **THEN** the file is created with `ft.synth.enabled: true` frontmatter and the scaffolded sections in result order; `$EDITOR` is launched at the bottom of the file

#### Scenario: Append to existing synth note dedups

- **WHEN** `ft notes synth Synthesis/topic.md --search "eigen"` is run and the file already exists with some matching paragraphs pinned
- **THEN** only the not-yet-pinned sections are appended (separated by a blank line) and `$EDITOR` is launched at the new bottom; existing content is preserved unchanged

#### Scenario: Re-running scaffold with the same query is idempotent

- **WHEN** `ft notes synth Synthesis/topic.md --search "eigen"` is run twice in succession with no source changes
- **THEN** the second run appends zero sections (all entries are already pinned)

#### Scenario: --no-edit suppresses editor handoff

- **WHEN** `ft notes synth ... --no-edit` is run
- **THEN** the file is written but `$EDITOR` is NOT launched and the command exits 0

#### Scenario: A source flag is required

- **WHEN** neither `--search`, `--link`, nor `--from` is passed
- **THEN** the command exits with a non-zero code and a clear "one of --search, --link, or --from is required" error

#### Scenario: Window flags are gone

- **WHEN** `--in-window`, `--since`, or `--range` is passed
- **THEN** the command fails with clap's unknown-argument error

### Requirement: Scaffold content sourcing

With `--search`, the scaffold SHALL be sourced from the paragraph-search index for the parsed query, deduplicated by `(source_path, line_start)`, with sections emitted in result order — relevance descending by default, newest-first with `--sort date` (see the paragraph-search capability). With the transitional `--link` form, sections SHALL be sourced from an any-mode search over the given links (paragraphs mentioning any link qualify; Related-alias resolution is not performed on this path). With `--from <path>:<line>` (repeatable), the scaffold SHALL additionally include the specified source paragraphs (identified by the line in which they start). Sections in the resulting scaffold SHALL be ordered by the search result order (which the `--sort` flag controls).

The scaffold's per-section body text SHALL be taken verbatim from the source paragraph's text (`ParagraphData.text` / `ParsedFile.paragraphs`). Because the heading line remains part of the paragraph that begins at that line (Fork A2), the body is unchanged in shape: a paragraph that begins at a heading line still includes the heading line verbatim.

#### Scenario: --search sources from the index
- **WHEN** `ft notes synth out.md --search "eigen memoization"` is run
- **THEN** the scaffold includes a section for every paragraph matching both terms, in relevance order

#### Scenario: --search --sort date orders newest first
- **WHEN** `ft notes synth out.md --search "eigen" --sort date` is run
- **THEN** the newest-edited matching paragraph appears first

#### Scenario: --from picks specific paragraphs
- **WHEN** `ft notes synth out.md --search "eigen" --from notes/bar.md:42 --no-edit` is run
- **THEN** the scaffold includes the search results PLUS the paragraph starting at line 42 of `notes/bar.md`

#### Scenario: Paragraph beginning at a heading line includes the heading
- **WHEN** a sourced paragraph begins at a `## Section` heading line
- **THEN** the scaffolded callout body begins with `## Section` (the heading line is part of the paragraph text, per Fork A2)

## REMOVED Requirements

### Requirement: Self-describing synth note targets
**Reason**: The capability existed to let `ft notes synth grow` re-run without `--link`. `grow` is removed, and scaffold no longer writes the key; search queries are re-runnable directly and append-dedup makes re-runs idempotent, so the persisted-targets mechanism has no remaining consumer.
**Migration**: Notes that already carry the key remain valid (the key is inert); verify/repair/reslice ignore it. Re-run scaffolding with `--search "<query>"` instead of `grow`.
