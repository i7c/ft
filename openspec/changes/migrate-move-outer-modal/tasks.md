## 1. New `Modal` impl for `GraphMoveOuter`

- [ ] 1.1 Lift `handle_move_key` (~217 LoC) from `tabs/graph.rs` into `impl Modal for GraphMoveOuter` in `tabs/graph.rs`. Split into per-variant private methods (`handle_source_from_tree`, `handle_source_picker`, `handle_inner`, `handle_target_from_tree`, `handle_target_picker`, `handle_move_target_from_tree`, `handle_move_target_picker`) called from the top-level `match self` to keep each branch ~30 LoC
- [ ] 1.2 Map `EventOutcome::Consumed` → `ModalOutcome::Consumed`; replace `self.move_outer = Some(next)` with `ModalOutcome::OpenSibling(Box::new(ActiveModal::MoveOuter(next)))`; replace direct method calls that need graph/view state with `AppRequest::GraphMove*` posts + `ModalOutcome::Closed`
- [ ] 1.3 Implement `Modal::render` for `GraphMoveOuter` by lifting the 7-arm render match. Keep the `mem::replace` trick for `SourcePicker` / `TargetPicker` so `notes_view::render_move_overlay` can be reused
- [ ] 1.4 Implement `Modal::keymap_help` per variant; `name()` returns `"move"` regardless of variant
- [ ] 1.5 Delete the stub `Modal for GraphMoveOuter` impl in `modal.rs`

## 2. New AppRequest variants + Tab hooks

- [ ] 2.1 Add `AppRequest::GraphMoveConfirmSourceFromTree`
- [ ] 2.2 Add `AppRequest::GraphMoveConfirmTargetFromTree`
- [ ] 2.3 Add `AppRequest::GraphMoveConfirmMoveTarget { selected: HashSet<NoteId> }`
- [ ] 2.4 Update the manual `Debug` impl on `AppRequest` for the three new variants
- [ ] 2.5 Add `Tab::graph_move_confirm_source_from_tree(&mut self, _ctx: &TabCtx)` default no-op
- [ ] 2.6 Add `Tab::graph_move_confirm_target_from_tree(&mut self, _ctx: &TabCtx)` default no-op
- [ ] 2.7 Add `Tab::graph_move_confirm_move_target(&mut self, _ctx: &TabCtx, _selected: HashSet<NoteId>)` default no-op

## 3. GraphTab implementations

- [ ] 3.1 Implement `graph_move_confirm_source_from_tree`: call `selected_note_hit` + `advance_to_multiselect`, post `OpenModal(MoveOuter(Inner(state)))` on success; toast + no-op on non-Note
- [ ] 3.2 Implement `graph_move_confirm_target_from_tree`: refactor existing `confirm_target_from_tree` to take `(ctx, carry)` and post `OpenModal(MoveOuter(Inner(Composing(...))))` on success
- [ ] 3.3 Implement `graph_move_confirm_move_target`: refactor existing `confirm_move_target` to take `(ctx, selected)`; refresh graph on success
- [ ] 3.4 Service the three new AppRequest variants in `App::service_request`, `service_pending_for_test`, `service_request_for_test`, and `drain_simple_requests`

## 4. Strip GraphTab fields and dispatch

- [ ] 4.1 Remove `move_outer: Option<GraphMoveOuter>` field
- [ ] 4.2 Remove the constructor init
- [ ] 4.3 Remove the `if self.move_outer.is_some()` dispatch arm
- [ ] 4.4 Remove the variant-specific render arm
- [ ] 4.5 Rewire all `m` key arms to post `OpenModal(MoveOuter(SourceFromTree))` or `OpenModal(MoveOuter(MoveTargetFromTree { selected }))` (Flow A)
- [ ] 4.6 Delete `handle_move_key` (logic now in `Modal::handle_event`)
- [ ] 4.7 Default: keep helpers (`confirm_target_from_tree`, `confirm_move_target`, `apply_inner_step`, `selected_note_hit`, `open_source_picker`) on GraphTab — they touch graph + view state; modal calls them via the new hooks

## 5. Inner-step intercept

- [ ] 5.1 In `handle_inner`, call `SectionMoveState::handle_event` on the inner; inspect its `ModalOutcome`
- [ ] 5.2 If the inner returns a transition to `TargetPicking`, intercept and return `OpenSibling(MoveOuter(TargetFromTree { carry }))` instead of forwarding
- [ ] 5.3 Otherwise forward the inner's outcome transparently (`Consumed` → `Consumed`, `Closed` → `Closed`, `OpenSibling(other)` → `OpenSibling(other)`)
- [ ] 5.4 Drop `apply_inner_step` if its only caller is the new modal

## 6. Tests

- [ ] 6.1 Run every existing move-flow integration test (`graph_m_*`, `graph_r_on_multi_selected_*`, etc.) — must pass with at most status-bar diffs
- [ ] 6.2 Re-bless status-bar snapshots that now show `modal: move` during move flows
- [ ] 6.3 Manual spot-check: open Graph tab, press `m`, verify `modal: move` indicator; walk the full flow

## 7. Build validation

- [ ] 7.1 `cargo build --release` — clean
- [ ] 7.2 `cargo test --workspace` — all tests pass; snapshot diffs limited to deliberate `modal: move` indicator
- [ ] 7.3 `cargo clippy --workspace --tests -- -D warnings` — clean
- [ ] 7.4 `cargo fmt --check` — clean
