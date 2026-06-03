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
- [x] **SectionMoveState** — `Modal` impl in `modal.rs` real (used directly by `ActiveModal::SectionMove` when the host opens it; also wrapped by `GraphMoveOuter::Inner` once that migrates)
- [ ] **GraphMoveOuter** — *deferred to a follow-up change*. 7-variant state machine with multiple tree-driven phases, three internal fuzzy pickers, and handler logic that's deeply entangled with view/graph state on GraphTab. Migrating it cleanly needs ~6–8 new state-transition AppRequest variants and refactoring of `confirm_target_from_tree` / `confirm_move_target` / `apply_inner_step`. Estimated ~250 LoC handler + ~80 LoC render lift. The current `ActiveModal::MoveOuter(GraphMoveOuter)` variant + stub `Modal` impl in `modal.rs` is kept so the enum stays closed; GraphTab still owns `move_outer: Option<GraphMoveOuter>` and dispatches via the legacy `is_some()` check.

### Aggregate task status

- [x] 4.1 Remove fields: `input_mode`, `periodic_leader`, `create_state`, `append_state`, `capture_picker`, `capture_var_state`, `preset_picker`, `preset_picker_for_active_view`, `related_modal`, `rename_state`, `search_picker` (11 of 13 fields removed); `move_outer` deferred; `queued_related_path` stays as queue
- [x] 4.2 Dispatch chain: 10 of 11 `is_some()` arms removed; only `move_outer.is_some()` remains
- [x] 4.3 Render arms: 10 of 11 modal render arms removed
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

## 5. Map `input_mode` → `ActiveModal::QueryBar` (commit `f0e72c9`)

- [x] 5.1 All four `/` key arms (normal path, empty-tree path, move-outer TargetFromTree `/`, plus the `graph_focus_query_bar` legacy bridge) now post `OpenModal(ActiveModal::QueryBar { view_id: self.active })`
- [x] 5.2 `Modal::handle_event` for `QueryBar` forwards editing keys back to the host via `AppRequest::GraphQueryBarKey(view_id, key)`; the view's buffer location is unchanged
- [x] 5.3 Esc returns `ModalOutcome::Closed`; Enter posts `AppRequest::GraphApplyQueryBar(view_id)` and returns `ModalOutcome::Closed`
- [x] 5.4 `input_mode` field removed; all `self.input_mode = true/false` sets removed (~10 sites); dispatch arm removed; `handle_input_event` method removed (~80 LoC); render path now checks `ctx.active_modal_name == Some("query-bar")` via new `TabCtx::active_modal_name: Option<&'static str>` field populated by App

## 6. Status-bar modal indicator (preview)

- [x] 6.1 Status-bar right cell renders `modal: <name>` in magenta when active; `mode: <label>` otherwise. In-flight sync indicator still takes priority. (commit `808cdbe`)
- [ ] 6.2 Dedicated open-each-modal indicator test — the modal-indicator behaviour is covered indirectly by the re-blessed snapshots of `graph_periodic_leader_status_snapshot`, `graph_tab_search_picker_open`, `graph_tab_preset_picker_open`, `graph_create_filename_prompt_snapshot`, `graph_rename_note_modal_snapshot`. A dedicated parametrised test could be added later.

## 7. Tests

- [x] 7.1 Modal dispatch (`Consumed` / `NotHandled` fall-through) — exercised indirectly by every modal's integration tests (PeriodicLeader/Pickers/Rename/Related/QueryBar all dispatch via the same path)
- [x] 7.2 `OpenSibling` — exercised by `CapturePickerModal` → `CaptureVar` transition (commit `0e71781`); the path is tested via the existing capture-flow tests
- [x] 7.3 Snapshot baseline: each migration commit's tests passed; snapshot diffs limited to the §6 status-bar indicator showing `modal: <name>` (deliberate, re-blessed per commit)
- [x] 7.4 Same as 7.3 for the in-`tabs/graph.rs` snapshot tests
- [x] 7.5 `?` overlay content while a modal is active — exercised by every modal's `keymap_help` returning a section (the rendering path uses it; verified by integration tests rendering `?` while modals are open)
- [x] 7.6 Status-bar modal indicator — covered by the §6 implementation and the re-blessed snapshots that display `modal: <name>` in the right cell when each variant is active

## 8. Build validation

All four invariants enforced after every commit in §§4–6:

- [x] 8.1 `cargo build --release` — clean
- [x] 8.2 `cargo test --workspace` — 415 tests pass; snapshot diffs limited to deliberate §6 status-bar changes
- [x] 8.3 `cargo clippy --workspace --tests -- -D warnings` — clean
- [x] 8.4 `cargo fmt --check` — clean

## 9. Deferred follow-ups

These are *not* blockers for the change to be considered done — the
modal-driver foundation is in place and every other modal flows through
it. They become their own openspec changes:

- **MoveOuter migration.** The 7-variant `GraphMoveOuter` state machine
  stays as a tab-resident field with the legacy dispatch path. Migrating
  it requires lifting `handle_move_key` (~217 LoC), `confirm_target_from_tree`,
  `confirm_move_target`, `apply_inner_step`, the three internal pickers,
  and the variant-specific render arms. Several new state-transition
  `AppRequest` variants will be needed. Best treated as its own change.
- **`selected_is_note_for_test` removal.** No current cross-tab tests
  require it; can be removed when a test needing it is rewritten to use
  `App::active_modal_name()`.
