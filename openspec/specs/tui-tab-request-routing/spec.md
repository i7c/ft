# tui-tab-request-routing Specification

## Purpose

TBD - created by syncing change graph-tab-decomposition. Update Purpose
after archive.

The single typed `GraphRequest` payload + `Tab::handle_graph_request`
mechanism that App uses to route Graph-targeted, modal-raised requests to
the tab that owns `TabKind::Graph`, replacing the one-hook-per-action
pattern (`AppRequest::Graph*` variants + `Tab::graph_*` methods).

## Requirements

### Requirement: A single typed enum carries every Graph-targeted cross-tab request

Cross-tab and modal-raised requests that must be serviced by the tab identified by `TabKind::Graph` SHALL be carried as a single `GraphRequest` enum, wrapped in one `AppRequest::Graph(GraphRequest)` variant. `AppRequest` SHALL NOT contain more than one variant dedicated to routing a request to the Graph tab.

#### Scenario: A rename commit is carried as a `GraphRequest` variant
- **WHEN** the rename modal commits a rename on Enter
- **THEN** it posts `AppRequest::Graph(GraphRequest::CommitRename { note_id, is_directory, source_rel, new_name })`

#### Scenario: A task-edit popup commit is carried as a `GraphRequest` variant
- **WHEN** the task-edit popup commits validated fields
- **THEN** it posts `AppRequest::Graph(GraphRequest::TaskEdit { path, line, fields })`

### Requirement: One `Tab` method dispatches every Graph-targeted request

The `Tab` trait SHALL expose exactly one method, `handle_graph_request(&mut self, req: GraphRequest, ctx: &mut TabCtx)`, for receiving requests routed to the Graph tab. Its default implementation SHALL be a no-op, matching the convention used by other default-no-op `Tab` methods. The `Tab` trait SHALL NOT expose per-action methods (e.g. `graph_commit_rename`, `graph_move_confirm_source_from_tree`, `graph_task_edit`) for this purpose.

#### Scenario: Non-Graph tabs use the default no-op
- **WHEN** a `GraphRequest` is routed to a tab that is not `TabKind::Graph`
  (which cannot happen through normal App routing, since routing looks up
  the tab by `TabKind::Graph` before calling the method) or when a tab
  other than `GraphTab` does not override `handle_graph_request`
- **THEN** the default no-op body runs and no panic or observable effect
  occurs

#### Scenario: GraphTab dispatches by matching on `GraphRequest`
- **WHEN** `GraphTab::handle_graph_request` receives
  `GraphRequest::ConfirmDelete { target, is_directory }`
- **THEN** it invokes the same delete-commit logic previously reached via
  `graph_confirm_delete` (plan + apply delete, refresh the graph, toast
  the outcome) — behavior is unchanged, only the dispatch surface differs

### Requirement: The App routes Graph requests through one lookup arm

`App::service_simple` SHALL contain exactly one match arm for routing `AppRequest::Graph(GraphRequest)` values: it looks up the tab with `kind() == TabKind::Graph` and calls `handle_graph_request` on it. It SHALL NOT contain a separate match arm per `GraphRequest` variant.

#### Scenario: Any `GraphRequest` variant reaches the Graph tab through the same arm
- **WHEN** `service_simple` receives `AppRequest::Graph(GraphRequest::NavigatePeriodic(period))`
- **THEN** the same lookup-and-call code path handles it as would handle
  `AppRequest::Graph(GraphRequest::CreateSubdir { .. })` — one arm, not
  one arm per variant

#### Scenario: Terminal-touching requests are unaffected
- **WHEN** `service_simple`/`service_request` processes `AppRequest::OpenInEditor`, `OpenInObsidian`, `SyncGit`, or `CommitGit`
- **THEN** it is handled exactly as before this change — these were never
  `Graph*` variants and this change does not alter their dispatch
