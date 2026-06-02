## ADDED Requirements

### Requirement: Quick capture preset configuration
The system SHALL support a `[capture_presets.<name>]` config section where each preset defines an action (`append` or `create`), a template name, and optional target resolution fields. Unknown keys in a preset SHALL be rejected at config load time.

#### Scenario: Append preset with hardcoded note
- **WHEN** the config contains `[capture_presets.journal]\naction = "append"\ntemplate = "daily.md"\nnote = "Journal/daily.md"`
- **THEN** invoking the `journal` preset SHALL append the `daily.md` template to `Journal/daily.md` without prompting for file or template

#### Scenario: Create preset with path pattern
- **WHEN** the config contains `[capture_presets.meeting]\naction = "create"\ntemplate = "meeting.md"\npath = "%Y-%m-%d meeting"\nfolder = "Meetings"`
- **THEN** invoking the `meeting` preset SHALL create a note at `Meetings/2026-06-01 meeting.md` (for today's date) from the `meeting.md` template

#### Scenario: Preset with unknown key rejected
- **WHEN** the config contains `[capture_presets.bad]\naction = "append"\ntemplate = "t.md"\nnot_a_key = "x"`
- **THEN** config loading SHALL fail with an error naming the unknown key

#### Scenario: Preset missing required field rejected
- **WHEN** the config contains `[capture_presets.incomplete]\naction = "append"` (no `template`)
- **THEN** config loading SHALL fail with an error indicating `template` is required

#### Scenario: Preset with invalid action rejected
- **WHEN** the config contains `[capture_presets.bad]\naction = "invalid"\ntemplate = "t.md"`
- **THEN** config loading SHALL fail with an error indicating `action` must be `append` or `create`

### Requirement: Append preset target resolution
An append preset SHALL resolve its target note from: (1) the `note` field in the preset config (hardcoded), or (2) the user's current selection in the invoking tab context (graph tab: selected note; notes tab: vault file picker if no selection).

#### Scenario: Hardcoded note takes precedence
- **WHEN** the preset has `note = "Areas/finance.md"` and the user is on the graph tab with a different note selected
- **THEN** the template SHALL be appended to `Areas/finance.md`, ignoring the selected note

#### Scenario: Graph tab selection fallback
- **WHEN** the preset has no `note` field and the user invokes it from the graph tab with `Projects/todo.md` selected
- **THEN** the template SHALL be appended to `Projects/todo.md`

#### Scenario: Notes tab opens file picker
- **WHEN** the preset has no `note` field and the user invokes it from the notes tab idle state (any existing picker state is dismissed)
- **THEN** the system SHALL open a fresh vault file picker; when the user selects a note, the template SHALL be appended to it

#### Scenario: Preset specifies section override
- **WHEN** the preset has `section = "Daily Log"` and the target note has `ft-append-section: Weekly Log` in frontmatter
- **THEN** the template SHALL be appended after the `Daily Log` section, ignoring the frontmatter value

### Requirement: Create preset target resolution
A create preset SHALL resolve its target path from: (1) the `path` field with chrono strftime expansion plus `.md` suffix, placed under `folder` (vault-relative); (2) if `path` is absent, open a filename prompt under `folder` (or under a folder picker if `folder` is also absent). If the resolved path already exists, the preset SHALL overwrite it.

#### Scenario: Path pattern resolves to dated file
- **WHEN** the preset has `path = "session-%Y%m%d"` and `folder = "Sessions"` and today is 2026-06-01
- **THEN** the note SHALL be created at `Sessions/session-20260601.md`

#### Scenario: Path absent opens filename prompt
- **WHEN** the preset has `action = "create"`, `template = "meeting.md"`, and no `path` field
- **THEN** the system SHALL open a filename prompt (single-line text entry) and create the file there under `folder` (or root if `folder` is absent)

#### Scenario: Collision overwrites
- **WHEN** the preset's resolved path already exists on disk
- **THEN** the file SHALL be overwritten atomically with the rendered template content

### Requirement: Quick capture TUI invocation
From the graph tab, pressing `Q` (shift-q) SHALL open a fuzzy picker listing all `[capture_presets]` names. From the notes tab, pressing `Q` SHALL open the same picker. Selecting a preset name SHALL immediately execute the preset.

#### Scenario: Graph tab quick capture picker
- **WHEN** the user presses `Q` from the graph tab with two presets configured (`journal`, `meeting`)
- **THEN** a fuzzy picker SHALL appear showing `journal` and `meeting` as options

#### Scenario: Preset picker cancellation
- **WHEN** the user presses `Q` then `Esc` in the preset picker
- **THEN** the picker SHALL close and the tab SHALL return to its previous state

#### Scenario: Quick capture with create preset opens editor at last line
- **WHEN** the user invokes a create preset and the template renders to `# New\n\nbody\n`
- **THEN** the editor SHALL open at the last line of the newly created file, landing on the inserted content

#### Scenario: Quick capture with append preset opens editor at insertion line
- **WHEN** the user invokes an append preset and the template renders to `entry\n`
- **THEN** the editor SHALL open at the line where `entry\n` was inserted in the target file

### Requirement: Quick capture respects append template frontmatter
When a quick capture preset has `action = "append"` and no `section` override, the system SHALL read `ft-append-section` from the target note's frontmatter to determine the append location.

#### Scenario: Append preset without section uses frontmatter
- **WHEN** the preset has `action = "append"`, `template = "log.md"`, `note = "Journal/daily.md"`, no `section` field, and `Journal/daily.md` has `ft-append-section: Daily Log` in its frontmatter
- **THEN** the template SHALL be appended after the `Daily Log` section

#### Scenario: Append preset without section or frontmatter appends to end
- **WHEN** the preset has `action = "append"`, `template = "log.md"`, no `note` field (target resolved from graph tab selection), no `section` field, and the selected note has no `ft-append-section` frontmatter
- **THEN** the template SHALL be appended to the end of the selected note
