## MODIFIED Requirements

### Requirement: Frontmatter section configuration
The system SHALL read the append section from the nested `ft.append.section` YAML frontmatter key. Absence of the key SHALL default to end-of-file append. The legacy flat `ft-append-section` key SHALL NOT be recognized.

#### Scenario: Frontmatter specifies section

- **WHEN** the target file has frontmatter `---\nft:\n  append:\n    section: Daily Log\n---\n# Title\n## Daily Log\nentry\n` and the caller appends without an explicit section override
- **THEN** the template content SHALL be appended after the `## Daily Log` section

#### Scenario: Frontmatter absent defaults to end

- **WHEN** the target file has no frontmatter and the caller appends without an explicit section override
- **THEN** the template content SHALL be appended to the end of the file

#### Scenario: Legacy flat key is ignored

- **WHEN** the target file has frontmatter `---\nft-append-section: Daily Log\n---\n# Title\n` (legacy flat form) and no nested `ft:` map, and the caller appends without an explicit section override
- **THEN** the template content SHALL be appended to the end of the file (the legacy key is not recognized)

#### Scenario: Explicit section override takes precedence over frontmatter

- **WHEN** the target file has `ft.append.section: Daily Log` in frontmatter but the caller explicitly specifies section `Weekly Log`
- **THEN** the content SHALL be appended after `Weekly Log`, ignoring the frontmatter value
