## MODIFIED Requirements

### Requirement: A single typed enum carries every Graph-targeted cross-tab request

Cross-tab and modal-raised requests that must be serviced by the tab identified by `TabKind::Graph` SHALL be carried as a single `GraphRequest` enum, wrapped in one `AppRequest::Graph(GraphRequest)` variant. `AppRequest` SHALL NOT contain more than one variant dedicated to routing a request to the Graph tab.

The Tasks tab SHALL have its own parallel routing channel: a single `TasksRequest` enum, wrapped in one `AppRequest::Tasks(TasksRequest)` variant, carrying every modal-raised request that must be serviced by the tab identified by `TabKind::Tasks`. `AppRequest` SHALL NOT contain more than one variant dedicated to routing a request to the Tasks tab. This mirrors the Graph-tab channel rather than introducing a generic active-tab-routing abstraction.

#### Scenario: A rename commit is carried as a `GraphRequest` variant
- **WHEN** the rename modal commits a rename on Enter
- **THEN** it posts `AppRequest::Graph(GraphRequest::CommitRename { note_id, is_directory, source_rel, new_name })`

#### Scenario: A task-edit popup commit is carried as a `GraphRequest` variant
- **WHEN** the task-edit popup commits validated fields
- **THEN** it posts `AppRequest::Graph(GraphRequest::TaskEdit { path, line, fields })`

#### Scenario: A tasks preset apply is carried as a `TasksRequest` variant
- **WHEN** the tasks preset-picker modal commits a selected preset on Enter
- **THEN** it posts `AppRequest::Tasks(TasksRequest::ApplyPreset(dsl))`

### Requirement: One `Tab` method dispatches every Tasks-targeted request

The `Tab` trait SHALL expose a method `handle_tasks_request(&mut self, req: TasksRequest, ctx: &mut TabCtx)`, parallel to `handle_graph_request`, for receiving requests routed to the Tasks tab. Its default implementation SHALL be a no-op, matching the convention used by other default-no-op `Tab` methods. The `Tab` trait SHALL NOT expose per-action methods for Tasks-targeted requests.

#### Scenario: Non-Tasks tabs use the default no-op
- **WHEN** a `TasksRequest` is routed to a tab that is not `TabKind::Tasks` or when a tab other than `TasksTab` does not override `handle_tasks_request`
- **THEN** the default no-op body runs and no panic or observable effect occurs

#### Scenario: TasksTab dispatches by matching on `TasksRequest`
- **WHEN** `TasksTab::handle_tasks_request` receives `TasksRequest::ApplyPreset(dsl)`
- **THEN** it sets the active SearchView's query text to `dsl`, recompiles the query against `ctx.today`, and recomputes the matches list against the current shared snapshot (no graph rebuild)

### Requirement: The App routes Tasks requests through one lookup arm

`App::service_simple` SHALL contain exactly one match arm for routing `AppRequest::Tasks(TasksRequest)` values: it looks up the tab with `kind() == TabKind::Tasks` and calls `handle_tasks_request` on it. It SHALL NOT contain a separate match arm per `TasksRequest` variant. This arm is a parallel of the existing `AppRequest::Graph` arm; the two channels are independent and a Tasks-targeted request SHALL NOT be routed to the Graph tab or vice versa.

#### Scenario: Any `TasksRequest` variant reaches the Tasks tab through the same arm
- **WHEN** `service_simple` receives `AppRequest::Tasks(TasksRequest::ApplyPreset(dsl))`
- **THEN** the same lookup-and-call code path handles it as would handle any future `TasksRequest` variant — one arm, not one arm per variant

#### Scenario: Graph-targeted routing is unchanged
- **WHEN** `service_simple` processes `AppRequest::Graph(GraphRequest::ApplyPreset(_))`
- **THEN** it is routed to the Graph tab exactly as before this change (the Graph tab's own `Ctrl+P` flow is unaffected by the new Tasks channel)
