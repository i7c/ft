## MODIFIED Requirements

### Requirement: Append preset target resolution
An append preset SHALL resolve its target note from: (1) the `note` field in the preset config (hardcoded), or (2) the user's current selection in the invoking tab context (graph tab: selected note; notes tab: a vault file picker). When no target can be resolved synchronously (notes tab, no `note` field), the capture flow SHALL return a `NeedsTarget` continuation rather than an error, so the caller can open a vault file picker and resume the flow once a note is chosen.

#### Scenario: Hardcoded note takes precedence
- **WHEN** the preset has `note = "Areas/finance.md"` and the user is on the graph tab with a different note selected
- **THEN** the template SHALL be appended to `Areas/finance.md`, ignoring the selected note

#### Scenario: Graph tab selection fallback
- **WHEN** the preset has no `note` field and the user invokes it from the graph tab with `Projects/todo.md` selected
- **THEN** the template SHALL be appended to `Projects/todo.md`

#### Scenario: Notes tab opens file picker
- **WHEN** the preset has no `note` field and the user invokes it from the notes tab idle state (any existing picker state is dismissed)
- **THEN** the system SHALL open a fresh vault file picker; when the user selects a note, the template SHALL be appended to it

#### Scenario: Notes tab file picker cancellation
- **WHEN** the notes tab file picker is open and the user presses `Esc`
- **THEN** the picker SHALL close and the tab SHALL return to idle with no append performed and no error toast

#### Scenario: Notes tab file picker precedes var prompt
- **WHEN** the preset has no `note` field, its template references `{{ vars.topic }}`, and the user invokes it from the notes tab
- **THEN** the system SHALL open the vault file picker first, and after a note is selected SHALL transition into the var prompt for `topic` before committing

#### Scenario: Preset specifies section override
- **WHEN** the preset has `section = "Daily Log"` and the target note has `ft-append-section: Weekly Log` in frontmatter
- **THEN** the template SHALL be appended after the `Daily Log` section, ignoring the frontmatter value

### Requirement: Quick capture TUI invocation
From the graph tab, pressing `Q` (shift-q) SHALL open a fuzzy picker listing all `[capture_presets]` names. From the notes tab, pressing `Q` SHALL open the same picker. Selecting a preset name SHALL either execute the preset immediately, transition into a var prompt (template has `{{ vars.* }}` references), or — for append presets with no `note` field invoked from the notes tab — transition into a vault file picker to choose the target note.

#### Scenario: Graph tab quick capture picker
- **WHEN** the user presses `Q` from the graph tab with two presets configured (`journal`, `meeting`)
- **THEN** a fuzzy picker SHALL appear showing `journal` and `meeting` as options

#### Scenario: Preset picker cancellation
- **WHEN** the user presses `Q` then `Esc` in the preset picker
- **THEN** the picker SHALL close and the tab SHALL return to its previous state

#### Scenario: Notes tab append preset without note opens file picker
- **WHEN** the user presses `Q` from the notes tab and selects an append preset that has no `note` field
- **THEN** a vault file picker SHALL open (not an error toast); selecting a note SHALL append the rendered template to it

#### Scenario: Quick capture with create preset opens editor at last line
- **WHEN** the user invokes a create preset and the template renders to `# New\n\nbody\n`
- **THEN** the editor SHALL open at the last line of the newly created file, landing on the inserted content

#### Scenario: Quick capture with append preset opens editor at insertion line
- **WHEN** the user invokes an append preset and the template renders to `entry\n`
- **THEN** the editor SHALL open at the line where `entry\n` was inserted in the target file
