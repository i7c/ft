## MODIFIED Requirements

### Requirement: `GraphMoveOuter` flows through `ActiveModal`

The `GraphMoveOuter` state SHALL be wrapped in `ActiveModal::MoveOuter(GraphMoveOuter)` and managed by the App's `active_modal` slot. The `move_outer: Option<GraphMoveOuter>` field on `GraphTab` SHALL be removed. The `if self.move_outer.is_some()` dispatch arm in `GraphTab::handle_event` SHALL be removed. The variant-specific render arm in `GraphTab::render` SHALL be removed.

#### Scenario: `m` posts OpenModal
- **WHEN** the user presses `m` on the Graph tab with no other modal active
- **THEN** the tab posts `AppRequest::OpenModal(Box::new(ActiveModal::MoveOuter(GraphMoveOuter::SourceFromTree)))` and returns `EventOutcome::Consumed`

#### Scenario: dispatch routes through modal driver
- **WHEN** `ActiveModal::MoveOuter(...)` is the active modal
- **THEN** App's modal-first dispatch routes key events to `GraphMoveOuter::handle_event`; the tab's `handle_event` is not called for those keys

#### Scenario: status-bar indicator shows `modal: move`
- **WHEN** any variant of `GraphMoveOuter` is the active modal
- **THEN** `App::active_modal_name()` returns `Some("move")` and the status bar's right cell renders `modal: move` in magenta

### Requirement: Internal pickers transition via `OpenSibling`

When an internal picker (`SourcePicker`, `TargetPicker`, `MoveTargetPicker`) returns `PickerOutcome::Selected(item)`, the `GraphMoveOuter::handle_event` SHALL return `ModalOutcome::OpenSibling(Box::new(ActiveModal::MoveOuter(<next variant>)))` to advance the state machine.

#### Scenario: SourcePicker selection advances to Inner
- **WHEN** `MoveOuter::SourcePicker { picker }` is active and the user picks a file
- **THEN** the modal returns `OpenSibling` carrying `MoveOuter(Inner(advance_to_multiselect(hit)))` and the App swaps the slot in one event-loop iteration

#### Scenario: SourcePicker cancel returns to SourceFromTree
- **WHEN** `MoveOuter::SourcePicker { picker }` is active and the user presses Esc
- **THEN** the modal returns `OpenSibling(MoveOuter(SourceFromTree))`, not `Closed` (the user can still confirm a source from the tree)

### Requirement: State-touching commits route via `AppRequest`

Move-flow actions that need to read the host's in-memory graph or mutate view state SHALL be expressed as `AppRequest::GraphMove*` variants routed through `Tab::graph_move_*` hooks. The host's existing `confirm_target_from_tree`, `confirm_move_target`, `apply_inner_step` helper methods SHALL stay on `GraphTab` and be called from the hook implementations.

#### Scenario: confirm source from tree
- **WHEN** `MoveOuter::SourceFromTree` is active and the user presses `m`
- **THEN** the modal posts `AppRequest::GraphMoveConfirmSourceFromTree` and returns `ModalOutcome::Closed`; the App's `service_request` calls `Tab::graph_move_confirm_source_from_tree(ctx)` on the Graph tab, which (if the selection is a valid note) posts a follow-up `OpenModal(MoveOuter(Inner(HeadingMultiSelect)))` to continue the flow

#### Scenario: confirm Flow A target directory
- **WHEN** `MoveOuter::MoveTargetFromTree { selected }` is active and the user presses Enter or `m`
- **THEN** the modal posts `AppRequest::GraphMoveConfirmMoveTarget { selected }` and returns `ModalOutcome::Closed`; the host runs `plan_multi_rename` + `apply_rename_plan`, refreshes the graph, and toasts the outcome

## REMOVED Requirements

### Requirement: GraphMoveOuter is explicitly out of scope

**Replaced by the requirements above.** The prior `extract-modal-driver` change required `GraphMoveOuter` to remain a tab-resident field with the legacy dispatch path. This change supersedes that requirement: `GraphMoveOuter` now flows through the modal driver like every other modal.
