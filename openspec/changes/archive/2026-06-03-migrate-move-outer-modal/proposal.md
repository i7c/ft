## Why

`extract-modal-driver` migrated 11 of 12 modal variants through the App-level `ActiveModal` slot but explicitly deferred `GraphMoveOuter` — the 7-variant move-section state machine. It's the one remaining tab-resident modal in `GraphTab` (`move_outer: Option<GraphMoveOuter>` field + legacy `is_some()` dispatch + variant-specific render arm).

Three reasons to migrate it now rather than leave it indefinitely:

1. **Architectural consistency.** Every other modal flows through the driver. `move_outer` is the lone exception, and its presence keeps a stale "Tab-resident vs Modal" mental model alive. Future contributors will copy the wrong pattern.
2. **Status-bar modal indicator gap.** Today the move flow has no `modal: <name>` indicator because it never enters `active_modal`. Users in mid-flow have weaker UX feedback than in any other modal.
3. **The patterns are now established.** This session resolved every design question (picker selection routing, tab-resident state location, render lift target, modal commit via AppRequest). The recipe is mechanical; deferring just postpones the work.

## What Changes

- New `tabs/graph.rs` module-internal `Modal` impl for `GraphMoveOuter`. Handler lifts `handle_move_key` verbatim (217 LoC currently). Render lifts the 7-variant render arm into `Modal::render`. The state machine's transitions stay internal to the modal.
- Three new picker-newtypes (similar to `SearchPickerModal`) for the move flow's internal pickers (`SourcePicker`, `TargetPicker`, `MoveTargetPicker`). Each handles `PickerOutcome::Selected` by transitioning the outer state via `ModalOutcome::OpenSibling`. The `Inner(SectionMoveState)` variant already has a working `Modal` impl from `extract-modal-driver`.
- New `AppRequest` variants for state-transition actions that touch GraphTab/view state the modal can't reach:
  - `GraphMoveConfirmSourceFromTree` — commit currently-selected note as source
  - `GraphMoveConfirmTargetFromTree` — commit currently-selected note as target
  - `GraphMoveConfirmMoveTarget` — Flow A: commit directory as target
  - `GraphMoveApplyInnerStep(MoveStep)` — apply a `MoveStep` returned by the shared `SectionMoveState` to the outer state
- New `Tab::graph_move_*` hooks (default no-op; `GraphTab` overrides). These call the existing `confirm_target_from_tree`, `confirm_move_target`, `apply_inner_step` helper methods which keep their logic in place.
- `GraphTab` loses: `move_outer` field, the `is_some()` dispatch arm, and the variant-specific render arm.
- `m` key arm posts `OpenModal(ActiveModal::MoveOuter(GraphMoveOuter::SourceFromTree))` instead of setting the field directly.
- The `ActiveModal::MoveOuter` variant becomes functional (was a stub in the prior change).

## Capabilities

### Modified Capabilities

- `tui-modal-driver`: extends to cover the `MoveOuter` variant. The "GraphMoveOuter is out of scope" requirement from the prior change is replaced with a positive requirement that `MoveOuter` flows through the driver like every other modal. The `modal: move` status-bar indicator now displays during move flows.

## Impact

- **Modified**: `ft/src/tui/tabs/graph.rs` — adds `impl Modal for GraphMoveOuter`, three picker newtypes, three `Tab::graph_move_*` overrides; removes the field, dispatch arm, render arm, and the wrapper `handle_move_key` (logic lifts into the trait impl).
- **Modified**: `ft/src/tui/tab.rs` — adds 4 `AppRequest::GraphMove*` variants + matching `Tab::graph_move_*` default-no-op hooks.
- **Modified**: `ft/src/tui/app.rs` — adds 4 service-request arms (in `service_request`, `service_pending_for_test`, `service_request_for_test`, and `drain_simple_requests`).
- **Modified**: `ft/src/tui/modal.rs` — removes the stub `Modal for GraphMoveOuter` impl (real impl now lives in `tabs/graph.rs`).
- **Tests**: existing move-flow tests must pass without semantic diffs. Status-bar snapshots for move-flow tests will diff (showing `modal: move` instead of `mode: normal`) and are deliberately re-blessed.
- All four build invariants stay green.
