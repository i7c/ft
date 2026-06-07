# tui-commands

## ADDED Requirements

### Requirement: Graph tab exposes `graph.delete` command

The Graph tab SHALL register a `graph.delete` command in `GRAPH_COMMANDS`. The command SHALL open the confirmation-delete modal (`opens_modal: true`) when invoked on a Note or Directory row, and SHALL show an error toast on Ghost, Task, or Paragraph rows.

#### Scenario: graph.delete appears in command registry

- **WHEN** `GRAPH_COMMANDS` is built
- **THEN** it contains a `CommandDef` named `graph.delete` with scope `Tab("graph")`, group `"Notes"`, and `opens_modal: true`

#### Scenario: graph.delete opens confirmation modal on Note

- **WHEN** `graph.delete` is dispatched and the focused row is a Note
- **THEN** an `AppRequest::OpenModal(ActiveModal::ConfirmDelete(...))` is posted with the note's path and kind

#### Scenario: graph.delete opens confirmation modal on Directory

- **WHEN** `graph.delete` is dispatched and the focused row is a Directory
- **THEN** an `AppRequest::OpenModal(ActiveModal::ConfirmDelete(...))` is posted with the directory path and kind

#### Scenario: graph.delete toasts on Ghost

- **WHEN** `graph.delete` is dispatched and the focused row is a Ghost
- **THEN** an error toast shows "cannot delete a ghost — it does not exist on disk" and no modal opens

#### Scenario: graph.delete toasts on Task

- **WHEN** `graph.delete` is dispatched and the focused row is a Task
- **THEN** an error toast shows "cannot delete a task node — delete the task in its source file" and no modal opens

### Requirement: Graph tab exposes `graph.create-subdir` command

The Graph tab SHALL register a `graph.create-subdir` command in `GRAPH_COMMANDS`. The command SHALL open the create-subdir prompt modal (`opens_modal: true`) when invoked on a Directory row, and SHALL show an error toast on other row types.

#### Scenario: graph.create-subdir appears in command registry

- **WHEN** `GRAPH_COMMANDS` is built
- **THEN** it contains a `CommandDef` named `graph.create-subdir` with scope `Tab("graph")`, group `"Notes"`, and `opens_modal: true`

#### Scenario: graph.create-subdir opens prompt modal on Directory

- **WHEN** `graph.create-subdir` is dispatched and the focused row is a Directory
- **THEN** an `AppRequest::OpenModal(ActiveModal::CreateSubdir(...))` is posted with the parent directory path

#### Scenario: graph.create-subdir toasts on Note

- **WHEN** `graph.create-subdir` is dispatched and the focused row is a Note
- **THEN** an error toast shows "select a directory first" and no modal opens

### Requirement: `graph.create-blank` creates ghost notes instantly

When `graph.create-blank` is dispatched on a Ghost row, the system SHALL immediately create the note file at the ghost's vault-relative path with default `# <title>` content and open it in the editor, without opening the multi-step create flow.

#### Scenario: c on Ghost creates note instantly

- **WHEN** `graph.create-blank` is dispatched and the focused row is a Ghost with raw path `projects/alpha`
- **THEN** the file `projects/alpha.md` is created with content `# alpha\n`
- **AND** the note opens in `$EDITOR`
- **AND** the graph refreshes
- **AND** no modal is opened (folder picker and filename prompt are skipped)

#### Scenario: c on non-Ghost still opens create flow

- **WHEN** `graph.create-blank` is dispatched and the focused row is a Note or Directory
- **THEN** the multi-step create flow opens with the folder pre-seeded from the selection (existing behavior unchanged)

### Requirement: `graph.create-from-template` creates ghost notes from template

When `graph.create-from-template` is dispatched on a Ghost row, the system SHALL open only the template picker, then on template selection immediately create the note at the ghost's vault-relative path (skipping folder picker and filename prompt).

#### Scenario: C on Ghost opens template picker

- **WHEN** `graph.create-from-template` is dispatched and the focused row is a Ghost with raw path `projects/alpha`
- **THEN** the template picker modal opens with `folder_seed = Some(parent_of("projects/alpha"))`
- **AND** on selecting a template, the file `projects/alpha.md` is created with the rendered template content and opened in `$EDITOR`
- **AND** no folder picker or filename prompt modals appear

#### Scenario: C on non-Ghost still opens full create flow

- **WHEN** `graph.create-from-template` is dispatched and the focused row is a Note or Directory
- **THEN** the full create flow opens (template picker → folder picker → filename → ...) (existing behavior unchanged)

### Requirement: ConfirmDelete modal exposes commands

The `ConfirmDelete` modal SHALL expose commands `confirm-delete.yes` and `confirm-delete.no` with corresponding `CommandDef` entries and keymap bindings.

#### Scenario: ConfirmDelete commands registered

- **WHEN** `ConfirmDelete::commands()` is called
- **THEN** it returns a slice containing `confirm-delete.yes` (group "Flow", is_primary: true) and `confirm-delete.no` (group "Flow", is_primary: true)

#### Scenario: ConfirmDelete keymap

- **WHEN** the ConfirmDelete modal is active
- **THEN** `y` dispatches `confirm-delete.yes`, `n`/`Esc`/`q` dispatch `confirm-delete.no`, `Enter` dispatches the focused choice, `Left`/`h`/`Right`/`l` navigate between choices

### Requirement: CreateSubdir modal exposes commands

The `CreateSubdir` modal SHALL expose commands `create-subdir.confirm` and `create-subdir.cancel` with corresponding `CommandDef` entries and keymap bindings.

#### Scenario: CreateSubdir commands registered

- **WHEN** `CreateSubdir::commands()` is called
- **THEN** it returns a slice containing `create-subdir.confirm` (group "Flow", is_primary: true) and `create-subdir.cancel` (group "Flow", is_primary: true)

#### Scenario: CreateSubdir keymap

- **WHEN** the CreateSubdir modal is active
- **THEN** `Enter` dispatches `create-subdir.confirm`, `Esc` dispatches `create-subdir.cancel`
