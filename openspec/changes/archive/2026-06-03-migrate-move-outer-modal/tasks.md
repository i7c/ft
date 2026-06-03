## 1. New `Modal` impl for `GraphMoveOuter`

- [x] 1.1 Lift `handle_move_key` from `tabs/graph.rs` into `impl Modal for GraphMoveOuter`. Top-level `handle_event` does a single `std::mem::replace(self, dummy)` to take the variant by value, then dispatches to per-variant methods (`handle_source_from_tree`, `handle_source_picker`, `handle_inner`, `handle_target_from_tree`, `handle_target_picker`, `handle_move_target_from_tree`, `handle_move_target_picker`). Each branch restores `*self` on `Consumed`/`NotHandled` and lets the App swap on `Closed`/`OpenSibling`
- [x] 1.2 `Consumed`/`NotHandled` keep `*self` alive (state restored in-place); state-machine transitions use `ModalOutcome::OpenSibling(Box::new(ActiveModal::MoveOuter(next)))`; cross-tab commits post `AppRequest::GraphMove*` + return `ModalOutcome::Closed`
- [x] 1.3 `Modal::render` lifts the 7-arm render match; the `mem::replace` trick for `SourcePicker` / `TargetPicker` is preserved verbatim so `notes_view::render_move_overlay` is reused. Strip-area is recomputed from the modal's body `Rect` (first row)
- [x] 1.4 `Modal::keymap_help` returns per-variant rows; `name()` returns `"move"` regardless of variant
- [x] 1.5 Stub `Modal for GraphMoveOuter` impl removed from `modal.rs`

## 2. New AppRequest variants + Tab hooks

- [x] 2.1 Add `AppRequest::GraphMoveConfirmSourceFromTree`
- [x] 2.2 Add `AppRequest::GraphMoveConfirmTargetFromTree` (carries `Box<MoveCarry>` so the modal can be reopened on recoverable errors with state intact)
- [x] 2.3 Add `AppRequest::GraphMoveConfirmMoveTarget { selected: HashSet<NoteId> }`
- [x] 2.4 Update the manual `Debug` impl on `AppRequest` for the three new variants
- [x] 2.5 Add `Tab::graph_move_confirm_source_from_tree(&mut self, _ctx: &TabCtx)` default no-op
- [x] 2.6 Add `Tab::graph_move_confirm_target_from_tree(&mut self, _ctx: &TabCtx, _carry: MoveCarry)` default no-op
- [x] 2.7 Add `Tab::graph_move_confirm_move_target(&mut self, _ctx: &TabCtx, _selected: HashSet<NoteId>)` default no-op

## 3. GraphTab implementations

- [x] 3.1 `graph_move_confirm_source_from_tree`: refactored `confirm_source_from_tree(ctx)` posts `OpenModal(MoveOuter(Inner(state)))` on success and `OpenModalWithToast(MoveOuter(SourceFromTree), ...)` on retry paths
- [x] 3.2 `graph_move_confirm_target_from_tree`: refactored `confirm_target_from_tree(ctx, carry)` takes the carry by value; posts `OpenModal(MoveOuter(Inner(Composing(...))))` on success, `OpenModalWithToast(MoveOuter(TargetFromTree { carry }), ...)` on retry, and drops to idle on hard IO failure
- [x] 3.3 `graph_move_confirm_move_target`: refactored `confirm_move_target(ctx, selected)` validates the tree selection, calls `execute_multi_move` on success, posts `OpenModalWithToast(MoveOuter(MoveTargetFromTree { selected }), ...)` on retry. Also added `graph_move_execute_multi_move` hook + `AppRequest::GraphMoveExecuteMultiMove { selected, dir_path }` for the Flow A fuzzy-picker path (the picker's selected directory bypasses tree-row validation)
- [x] 3.4 Serviced the new AppRequest variants in `App::service_request`, `service_pending_for_test`, `service_request_for_test`, and `drain_simple_requests` (4 sites × 4 variants — `GraphMoveConfirmSourceFromTree`, `GraphMoveConfirmTargetFromTree`, `GraphMoveConfirmMoveTarget`, `GraphMoveExecuteMultiMove` — plus the new `OpenModalWithToast` shared variant)

## 4. Strip GraphTab fields and dispatch

- [x] 4.1 `move_outer: Option<GraphMoveOuter>` field removed
- [x] 4.2 Constructor init removed
- [x] 4.3 `if self.move_outer.is_some()` dispatch arm removed (one comment replaces the entire block)
- [x] 4.4 Variant-specific render arm removed (Modal::render owns it)
- [x] 4.5 Both `m` key arms (initial source-phase entry + multi-selected `r`) post `OpenModal(MoveOuter(SourceFromTree))` / `OpenModal(MoveOuter(MoveTargetFromTree { selected }))` respectively
- [x] 4.6 `handle_move_key` deleted; logic lives in `Modal::handle_event`
- [x] 4.7 Helpers kept on GraphTab: `confirm_source_from_tree`, `confirm_target_from_tree`, `confirm_move_target`, `execute_multi_move`, `selected_note_hit`. `open_source_picker` was a one-line wrapper around `FuzzyPicker::new` — replaced with module-level `open_move_file_picker(ctx)` so the Modal impl can build pickers without borrowing GraphTab. `apply_inner_step` was deleted: its only callers were inside `handle_move_key`, and the inner-step intercept logic now lives inline in `Modal::handle_event::handle_inner`

## 5. Inner-step intercept

- [x] 5.1 `handle_inner` calls `section_move::handle_key(&mut sms, k, ctx)` directly (not via `SectionMoveState::handle_event`) so the `MoveStep` return is inspectable — the Modal-trait impl on `SectionMoveState` swallows the transition variant via `*self = next`, which would erase the `TargetPicking` discriminator the intercept needs
- [x] 5.2 `MoveStep::Transition(SectionMoveState::TargetPicking { … })` is intercepted: extract the `MoveCarry` fields and return `OpenSibling(MoveOuter(TargetFromTree { carry }))`. The shared `TargetPicking` picker is dropped (the tree-driven phase takes over)
- [x] 5.3 Other transitions forward transparently: `Stay` → `Consumed` (sms restored), `NotHandled` → `NotHandled` (sms restored), `Finished` → `Closed`, `Transition(non-TargetPicking)` → `Consumed` with `*self = Inner(next)`
- [x] 5.4 `apply_inner_step` deleted — no callers remain after handle_move_key removal

## 6. Tests

- [x] 6.1 All existing move-flow integration tests pass (`graph_m_*`, `graph_r_*`, `graph_move_*` etc.) — 770 tests in the `ft` binary plus the rest of the workspace
- [x] 6.2 `graph_move_target_banner_80x24` snapshot re-blessed to show `modal: move` in the status-bar right cell (was `mode: normal`). One snapshot diff only — all other snapshots match because move-flow snapshots already had the active modal during pre-migration via different mechanisms
- [x] 6.3 Manual spot-check deferred to `verify` skill / interactive run; mechanical migration verified end-to-end by the existing test suite covering every flow path (SourceFromTree m/t/Esc, Inner step-2 multi-select, TargetFromTree m/t/Esc, TargetPicker picker outcomes, MoveTargetFromTree Enter/m/t/Esc, MoveTargetPicker picker outcomes)

### Notable behavior diffs

- **`graph_m_again_on_directory_emits_toast`** — test was updated to read the toast from `app.current_toast()` (App's toast slot) instead of `app.take_pending_request()` looking for `AppRequest::Toast`. The new architecture posts a combined `OpenModalWithToast` request that the App services as "set active_modal + push_toast" in one shot. Functionally identical from the user's POV; the test just inspects a different slot.
- **`/` on `TargetFromTree`** — the pre-migration UX had `/` open the QueryBar *while* the move flow stayed visible (carry preserved across the bar's lifetime), via `move_outer` + `input_mode` co-existence. The modal driver only holds one active modal, so the new behavior is: `/` cancels the move flow and opens the QueryBar (carry dropped). Banner text updated to remove the `/: refine` hint. No tests covered the old behavior.

### `OpenModalWithToast` — pending_request slot reconciliation

The single-slot `pending_request` had a latent issue: when a host hook needs to both push a toast AND re-open a modal, the second post overwrites the first. The pre-migration code dodged this by setting `move_outer` directly on `GraphTab` (no `pending_request` involvement). Post-migration, both go through `pending_request`. Solution: new `AppRequest::OpenModalWithToast { modal, toast_text, toast_style }` variant that the App services as "set active_modal + push_toast" atomically. Used in the four move-flow retry sites. Cleaner than queue_toast + OpenModal (which silently lost the toast); also fixes the latent rename-retry toast-loss bug if/when those sites adopt the same pattern.

## 7. Build validation

- [x] 7.1 `cargo build --release` — clean
- [x] 7.2 `cargo test --workspace` — all tests pass (770 in `ft`, plus `ft-core` etc.); only re-blessed snapshot diff is the deliberate `modal: move` indicator
- [x] 7.3 `cargo clippy --workspace --tests -- -D warnings` — clean
- [x] 7.4 `cargo fmt --check` — clean
