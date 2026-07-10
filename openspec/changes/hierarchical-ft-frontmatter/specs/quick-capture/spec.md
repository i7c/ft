## MODIFIED Requirements

### Requirement: Quick capture respects append template frontmatter
The system SHALL read the append section from the target note's nested `ft.append.section` frontmatter key when a quick capture preset has `action = "append"` and no `section` override. Absence of the key SHALL default to end-of-file append. The legacy flat `ft-append-section` key SHALL NOT be recognized.

#### Scenario: Append preset without section uses frontmatter

- **WHEN** the preset has `action = "append"`, `template = "log.md"`, `note = "Journal/daily.md"`, no `section` field, and `Journal/daily.md` has `ft:\n  append:\n    section: Daily Log` in its frontmatter
- **THEN** the template SHALL be appended after the `Daily Log` section

#### Scenario: Preset specifies section override

- **WHEN** the preset has `section = "Daily Log"` and the target note has `ft.append.section: Weekly Log` in frontmatter
- **THEN** the template SHALL be appended after the `Daily Log` section, ignoring the frontmatter value

#### Scenario: Append preset without section or frontmatter appends to end

- **WHEN** the preset has `action = "append"`, `template = "log.md"`, no `note` field (target resolved from graph tab selection), no `section` field, and the selected note has no `ft.append.section` frontmatter
- **THEN** the template SHALL be appended to the end of the selected note
