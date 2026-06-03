## 1. New `Modal` infrastructure

- [x] 1.1 Create `ft/src/tui/modal.rs` with the `ActiveModal` enum (variants: `Create`, `Append`, `CapturePicker`, `CaptureVar`, `MoveOuter`, `Rename`, `PresetPicker`, `Related`, `Search`, `PeriodicLeader`, `QueryBar`)
- [x] 1.2 Define the `Modal` trait (`handle_event`, `render`, `keymap_help`, `name`) and the `ModalOutcome` enum (`Consumed`, `Closed`, `OpenSibling(Box<ActiveModal>)`, `NotHandled`)
- [x] 1.3 Implement `Modal` for every `ActiveModal` variant — wrap the existing `handle_event` / `render` / `keymap_help` from each modal's source module (`notes_actions/create.rs`, `notes_actions/append.rs`, `notes_actions/capture.rs`, `notes_actions/section_move.rs`, `tabs/graph.rs` for the tab-specific ones)
- [x] 1.4 Add `impl<S: PickerSource> Modal for FuzzyPicker<S>` so the three picker variants share one implementation; map `PickerOutcome` → `ModalOutcome`

Notes from implementation:
- Tab-resident state types in `tabs/graph.rs` (`PresetPickerSource`, `GraphSearchPickerSource`, `GraphRenameState`, `RelatedModal`) were made `pub` so `ActiveModal` can wrap them. Field visibility is unchanged.
- `Modal::render` for variants without a free-function renderer (`CreateState`, `AppendState`, `SectionMoveState`, `CaptureVarPromptState`, the tab-resident variants, `PeriodicLeader`, `QueryBar`) is a stub today; the host tab still owns rendering. Section 4 lifts the render arms out of `tabs/graph.rs` into these impls.
- `FuzzyPicker::Modal::handle_event` is a placeholder that consumes the key but does not yet wire selection back to the host. The picker's typed `PickerOutcome<S::Item>` can't be erased to a bare `ModalOutcome` without knowing the host context; Section 4 routes selection through the same plumbing.
- `#![allow(dead_code)]` at the top of `modal.rs` keeps clippy quiet until Section 2 routes events through this module.

## 2. App-level slot + dispatch

- [x] 2.1 Add `active_modal: RefCell<Option<ActiveModal>>` to `App`; initialize `None` in `App::with_tabs`
- [x] 2.2 Add `App::active_modal_name(&self) -> Option<&'static str>` accessor for status-bar + tests
- [x] 2.3 In `App::handle_event`, before tab dispatch, call `active_modal.handle_event` and act on the returned `ModalOutcome` (consume / clear / replace / fall through)
- [x] 2.4 In `App::draw`, after the tab renders, render the active modal (if any) over the same body area
- [x] 2.5 In `App::enter_help`, when a modal is active, return the modal's `keymap_help()` instead of the tab's `help_sections()` (implemented in `App::draw`'s `Mode::Help` arm — the `enter_help` test helper just toggles mode)

## 3. `AppRequest::OpenModal` plumbing

- [x] 3.1 Add `AppRequest::OpenModal(ActiveModal)` variant in `ft/src/tui/tab.rs` (boxed: `OpenModal(Box<ActiveModal>)` per clippy `large_enum_variant`; `Clone` derive dropped on `AppRequest` since `ActiveModal` isn't `Clone`; manual `Debug` impl provided so `Option<AppRequest>` test assertions keep working)
- [x] 3.2 Service the request in `App::service_request` by writing into `active_modal` (also added to `service_pending_for_test` and `service_request_for_test`)
- [ ] 3.3 Replace the implicit `self.<modal> = Some(state)` lines in `GraphTab` with `ctx.pending_request.set(AppRequest::OpenModal(ActiveModal::<X>(state)))` *(deferred to Section 4 — coupled with field removal; doing 3.3 alone would create two sources of truth for the same modal state)*

## 4. Migrate `GraphTab` modal slots

### Per-modal migration progress

- [x] **PeriodicLeader** — fully migrated (commit `22cb319`)
- [x] **SearchPicker** (`SearchPickerModal` newtype) — `AppRequest::GraphJumpToNodes` (commit `3541a7b`)
- [x] **PresetPicker** (`PresetPickerModal` newtype) — `AppRequest::GraphApplyPreset` + `GraphFocusQueryBar` (commit `7a5c510`)
- [x] **CapturePicker** (`CapturePickerModal` newtype) + **CaptureVarPromptState** — first real use of `OpenSibling` (commit `0e71781`)
- [x] **CreateState**, **AppendState** — handlers in `notes_actions/*` already; render delegated to existing `notes_view::render_*_overlay` (commit `e9040a0`)
- [x] **GraphRenameState** — tab-resident `Modal` impl in `tabs/graph.rs`; commits via `AppRequest::GraphCommitRename`; re-opens on recoverable error (commit `02e477f`)
- [x] **RelatedModal** — tab-resident `Modal` impl in `tabs/graph.rs`; commits via `AppRequest::GraphConfirmRelated` (commit `fe11de1`)
- [ ] **GraphMoveOuter** — 7-variant state machine with multiple tree-driven phases and fuzzy pickers per phase. Most complex remaining. Needs its own Modal impl + likely several new AppRequest variants. ~250 LoC of handler logic + ~80 LoC render
- [ ] **SectionMoveState** — Modal impl already provided (used by `GraphMoveOuter::Inner`), but the outer wrapper isn't migrated yet

### Aggregate task status

- [x] 4.1 Remove fields: `periodic_leader`, `create_state`, `append_state`, `capture_picker`, `capture_var_state`, `preset_picker`, `preset_picker_for_active_view`, `related_modal`, `rename_state`, `search_picker` (10 of 13 fields removed); `input_mode` and `move_outer` remain; `queued_related_path` stays as queue
- [x] 4.2 Dispatch chain: 9 of 11 `is_some()` arms removed; only `move_outer.is_some()` and `input_mode` remain
- [x] 4.3 Render arms: 9 of 11 modal render arms removed
- [ ] 4.4 Replace `selected_is_note_for_test` with `App::active_modal_name()` in cross-tab tests *(deferred — no current tests need it)*

### Design decisions resolved

The picker selection routing problem (flagged in PeriodicLeader's notes) was resolved via **Strategy A**: new `AppRequest` variants per outcome (`GraphJumpToNodes`, `GraphApplyPreset`, `GraphFocusQueryBar`, `GraphCommitRename`, `GraphConfirmRelated`). The App finds the Graph tab by title and calls a typed `Tab::graph_*` hook. Same pattern is used for tab-resident commit flows.

Per-modal Render code:
- Flow modals (Create/Append/SectionMove/CaptureVar) → re-use existing `notes_view::render_*_overlay` functions (already `pub(crate)` in `tabs/notes/view.rs`)
- Picker modals → lifted into `Modal::render` on the newtype, calling `notes_view::render_picker_popup` or the existing search-picker chrome
- Tab-resident modals → render lifted from inline arm to `Modal::render` on the state struct, defined in `tabs/graph.rs`

Tab-resident state types (`GraphRenameState`, `RelatedModal`) host their `Modal` impls in `tabs/graph.rs` (the user's chosen Option for Q2 in the design-decision questionnaire).

### `drain_simple_requests` helper

Added an App-private helper that drains `OpenModal` + graph-routed back-action requests from `pending_request`. Called from `dispatch`, `switch_to`, and `apply_initial_action_for_test`. Production's main event loop continues to use `service_request` after each `handle_event`; this helper covers the test paths and the `on_focus` post-switch case where the modal-open needs to materialise before the next render.

## 5. Map `input_mode` → `ActiveModal::QueryBar`

- [ ] 5.1 `/` key arm raises `OpenModal(ActiveModal::QueryBar { view_id: active })`
- [ ] 5.2 `Modal::handle_event` for `QueryBar` reads/writes the active view's query buffer (unchanged buffer location)
- [ ] 5.3 Esc returns `ModalOutcome::Closed`; Enter applies the query and returns `ModalOutcome::Closed`
- [ ] 5.4 Remove the `input_mode` flag from `GraphTab`; remove all `if !self.input_mode` guards in the tree-key arms

## 6. Status-bar modal indicator (preview)

- [x] 6.1 Status-bar right cell renders `modal: <name>` in magenta when active; `mode: <label>` otherwise. In-flight sync indicator still takes priority. (commit `808cdbe`)
- [ ] 6.2 Dedicated open-each-modal indicator test — the modal-indicator behaviour is covered indirectly by the re-blessed snapshots of `graph_periodic_leader_status_snapshot`, `graph_tab_search_picker_open`, `graph_tab_preset_picker_open`, `graph_create_filename_prompt_snapshot`, `graph_rename_note_modal_snapshot`. A dedicated parametrised test could be added later.

## 7. Tests

- [ ] 7.1 Unit: `ModalDispatch` returns `Consumed` for a key the modal handles, lets others fall through
- [ ] 7.2 Unit: opening modal A then receiving `OpenSibling(B)` from A produces an active modal of B and clears A
- [ ] 7.3 Snapshot baseline: run `ft/src/tui/tests.rs` before migration, capture the snapshot inventory; re-run after migration, diff = zero
- [ ] 7.4 Snapshot baseline: same for `ft/src/tui/tabs/graph.rs` `mod tests`
- [ ] 7.5 New: integration test asserting `?` overlay content while a modal is active matches the modal's `keymap_help`, not the tab's
- [ ] 7.6 New: integration test asserting status-bar modal indicator renders the correct name when each `ActiveModal` variant is active

## 8. Build validation

- [ ] 8.1 `cargo build --release` — clean
- [ ] 8.2 `cargo test --workspace` — all tests pass, no snapshot diffs
- [ ] 8.3 `cargo clippy --workspace --tests -- -D warnings` — clean
- [ ] 8.4 `cargo fmt --check` — clean
