# append-template Specification

## Purpose
TBD - created by archiving change append-template-and-quick-capture. Update Purpose after archive.
## Requirements
### Requirement: Append template to end of file
The system SHALL render a template and append the result to the end of a target markdown file. If the file does not end with a newline, a `\n` SHALL be prepended before the template content.

#### Scenario: Append to file that ends with newline
- **WHEN** the target file contains `# Title\n\nbody text\n` and the template renders to `## New section\ncontent\n`
- **THEN** the file becomes `# Title\n\nbody text\n## New section\ncontent\n` and the editor opens at the line of `## New section`

#### Scenario: Append to file missing trailing newline
- **WHEN** the target file contains `# Title\nfinal line` (no trailing newline) and the template renders to `## Added\n`
- **THEN** the file becomes `# Title\nfinal line\n## Added\n` and the editor opens at the line of `## Added`

#### Scenario: Append to empty file
- **WHEN** the target file is empty and the template renders to `# First\n`
- **THEN** the file becomes `# First\n` and the editor opens at line 1

### Requirement: Append template to a specific section
The system SHALL support appending template output after the end of a named markdown section (heading + body). The section heading text SHALL be matched case-insensitively after trimming. The heading level SHALL be ignored for matching (any ATX level).

#### Scenario: Append after existing section
- **WHEN** the target file contains `# Intro\nintro body\n## Log\nlog line 1\nlog line 2\n## Footer\nfooter text\n` and the template renders to `log line 3\n` for section `Log`
- **THEN** the file becomes `# Intro\nintro body\n## Log\nlog line 1\nlog line 2\nlog line 3\n## Footer\nfooter text\n` and the editor opens at the line of `log line 3`

#### Scenario: Section heading not found
- **WHEN** the target file contains `# Title\nbody\n` and the caller requests append to section `Nonexistent`
- **THEN** the operation SHALL return an error indicating the section heading was not found

#### Scenario: Section heading matches different level
- **WHEN** the target file contains `### Sessions\nsession content\n` and the caller requests append to section `Sessions`
- **THEN** the content SHALL be appended after the `### Sessions` section body, regardless of the level being H3 rather than H2

#### Scenario: Multiple headings with same text
- **WHEN** the target file contains `## Log\na\n### Log\nb\n` and the caller requests append to section `Log`
- **THEN** the content SHALL be appended after the first matching section (the `## Log` section)

### Requirement: Frontmatter section configuration
The system SHALL read a `ft-append-section` key from YAML frontmatter to determine the target section for append operations. Absence of the key SHALL default to end-of-file append.

#### Scenario: Frontmatter specifies section
- **WHEN** the target file has frontmatter `---\nft-append-section: Daily Log\n---\n# Title\n## Daily Log\nentry\n` and the caller appends without an explicit section override
- **THEN** the template content SHALL be appended after the `## Daily Log` section

#### Scenario: Frontmatter absent defaults to end
- **WHEN** the target file has no frontmatter and the caller appends without an explicit section override
- **THEN** the template content SHALL be appended to the end of the file

#### Scenario: Explicit section override takes precedence over frontmatter
- **WHEN** the target file has `ft-append-section: Daily Log` in frontmatter but the caller explicitly specifies section `Weekly Log`
- **THEN** the content SHALL be appended after `Weekly Log`, ignoring the frontmatter value

### Requirement: CLI append subcommand
The system SHALL provide a `ft notes append` subcommand that accepts a target note path, a template path, an optional section heading, optional `--title` override, optional `--var` entries, and `--no-open`/`--editor` flags matching `ft notes create`.

#### Scenario: CLI append to end of file
- **WHEN** the user runs `ft notes append Areas/journal.md --template daily-log.md`
- **THEN** the template SHALL be rendered and appended to the end of `Areas/journal.md`, and the file SHALL be opened in `$EDITOR` at the insertion line

#### Scenario: CLI append with explicit section
- **WHEN** the user runs `ft notes append Areas/journal.md --template session.md --section "Sessions"`
- **THEN** the template SHALL be appended after the `Sessions` heading section in `Areas/journal.md`

#### Scenario: CLI append with --no-open
- **WHEN** the user runs `ft notes append Areas/journal.md --template daily.md --no-open`
- **THEN** the template SHALL be appended to the file but no editor SHALL be spawned

#### Scenario: CLI append target not found
- **WHEN** the user runs `ft notes append nonexistent.md --template t.md`
- **THEN** the command SHALL exit with a non-zero exit code and an error message indicating the target file does not exist

### Requirement: TUI append from graph tab
From the graph tab, pressing `A` (shift-a) SHALL open the template picker, and upon template selection SHALL append the rendered template to the currently selected note.

#### Scenario: Graph tab append with frontmatter section
- **WHEN** the selected note has `ft-append-section: Sessions` in its frontmatter and the user presses `A`, selects a template
- **THEN** the template SHALL be rendered and appended after the `Sessions` section in the selected note

#### Scenario: Graph tab append when no note is selected
- **WHEN** the graph tab has a directory node or empty state selected and the user presses `A`
- **THEN** the key SHALL be ignored (or a toast SHALL indicate "select a note first")

### Requirement: TUI append from notes tab
From the notes tab, pressing `a` SHALL open the template picker, then open the vault file picker for the target note, then append the rendered template.

#### Scenario: Notes tab append flow
- **WHEN** the user presses `a` from the notes tab idle state, selects a template, then selects a target note from the file picker
- **THEN** the template SHALL be rendered and appended to the selected note, and the editor SHALL open at the insertion line

