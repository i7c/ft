## MODIFIED Requirements

### Requirement: Promote from the Tasks tab

`tasks.promote` (`I`) SHALL promote the cursor task. Without a selected task it SHALL toast `select a task first`. When the cursor task has no eligible ticket parent, it SHALL toast the precondition message without any network call. When it has more than one, it SHALL first open the parent picker (`tui-tasks-link`) in choose mode, listing the eligible parents, and promote under the chosen one. `Esc` cancels without any network call.

Otherwise it SHALL run as a background job (`JobKind::Promote`, status-bar indicator `promote`):
- **Worker:** creates and fetches the remote issue.
- **Main thread:** plans and applies the local restructuring with the unchanged-file guard, raises a graph refresh, puts the cursor on the new anchor, and toasts `promoted to ABC-124`.

Failures SHALL toast the error, and SHALL include the created key when the remote side succeeded.

#### Scenario: Promote while browsing
- **WHEN** the user presses `I` on a linked subtask of a Jira story while online
- **THEN** the UI stays responsive, and when the job finishes the task appears as anchor `ABC-124` under the story, with its former children nested below

#### Scenario: Choose among two tickets
- **WHEN** the cursor task has `⤴ ABC-123,ABC-200` and the user presses `I`
- **THEN** a picker lists both tickets, and after choosing `ABC-200` the job promotes under `ABC-200`
