## MODIFIED Requirements

### Requirement: State-touching commits route via `AppRequest`

Move-flow actions that need to read the host's in-memory graph or mutate view state SHALL be expressed as `GraphRequest::Move*` payloads wrapped in `AppRequest::Graph(GraphRequest)` and routed through the single `Tab::handle_graph_request` hook. The host's existing `confirm_target_from_tree`, `confirm_move_target`, and `execute_multi_move` helper methods SHALL stay on `GraphTab` and be called from the `GraphRequest::Move*` match arms inside `handle_graph_request`.

#### Scenario: Confirm source from tree
- **WHEN** `MoveOuter::SourceFromTree` is active and the user presses `m`
- **THEN** the modal posts `AppRequest::Graph(GraphRequest::MoveConfirmSourceFromTree)` and returns `ModalOutcome::Closed`; the App's `service_simple` looks up the `TabKind::Graph` tab and calls `handle_graph_request`, whose `MoveConfirmSourceFromTree` arm (if the selection is a valid note) posts a follow-up `OpenModal(MoveOuter(Inner(HeadingMultiSelect)))` to continue the flow

#### Scenario: Confirm Flow A target directory
- **WHEN** `MoveOuter::MoveTargetFromTree { selected }` is active and the user presses Enter or `m`
- **THEN** the modal posts `AppRequest::Graph(GraphRequest::MoveConfirmMoveTarget { selected })` and returns `ModalOutcome::Closed`; the host runs `plan_multi_rename` + `apply_rename_plan`, refreshes the graph, and toasts the outcome

#### Scenario: Retry-with-toast uses `OpenModalWithToast`
- **WHEN** a host hook needs to atomically push a toast AND re-open the same modal (recoverable validation failure, e.g. wrong-row-type on confirm)
- **THEN** it posts `AppRequest::OpenModalWithToast { modal, toast_text, toast_style }` rather than separate `Toast` + `OpenModal` requests — the single-slot `pending_request` would otherwise lose one of the two; this is unchanged by the `GraphRequest` collapse, since `OpenModalWithToast` was never a `Graph*`-routed variant
