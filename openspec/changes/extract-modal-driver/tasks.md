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

- [x] **PeriodicLeader** — fully migrated end-to-end as proof of pattern (commit `22cb319`)
- [ ] **CreateState** — handler already in `notes_actions/create.rs`; needs render lift + `c`/`C` arm rewire
- [ ] **AppendState** — handler in `notes_actions/append.rs`; needs render lift + `A` arm rewire
- [ ] **SectionMoveState** — handler in `notes_actions/section_move.rs`; needs render lift + integration with `MoveOuter`
- [ ] **CaptureVarPromptState** — handler in `notes_actions/capture.rs`; needs render lift + `OpenSibling` from CapturePicker
- [ ] **CapturePresetPickerSource (FuzzyPicker)** — picker selection routing problem ⚠
- [ ] **PresetPickerSource (FuzzyPicker)** — picker selection routing problem ⚠
- [ ] **GraphSearchPickerSource (FuzzyPicker)** — picker selection routing problem ⚠
- [ ] **GraphRenameState** — tab-resident; needs handler + render lifted from `tabs/graph.rs`
- [ ] **RelatedModal** — tab-resident; needs handler + render lifted from `tabs/graph.rs`
- [ ] **GraphMoveOuter** — complex multi-phase; needs handler + render lifted

### Aggregate task status

- [ ] 4.1 Remove fields: ~~`periodic_leader`~~ done; `input_mode`, `create_state`, `append_state`, `capture_picker`, `capture_var_state`, `move_outer`, `rename_state`, `preset_picker`, `preset_picker_for_active_view`, `related_modal`, `search_picker` (12 of 13 still to migrate; `queued_related_path` stays)
- [ ] 4.2 Remove the `is_some()` dispatch chain — 1 of ~10 arms removed
- [ ] 4.3 Remove `render_*_overlay` calls — 1 of ~10 render arms removed
- [ ] 4.4 Replace `selected_is_note_for_test` with `App::active_modal_name()` in cross-tab tests

### Design issue surfaced during PeriodicLeader migration

**Picker selection routing.** `FuzzyPicker<S>::handle_event` returns a typed `PickerOutcome<S::Item>`. The host tab acts on `Selected(item)` (e.g. apply preset DSL, jump to node, open capture flow). When the picker becomes a Modal, that typed outcome cannot be erased into the bare `ModalOutcome` enum without losing the payload. Three options for the picker variants (Search / Preset / Capture):

1. **New `AppRequest` variants per outcome** (e.g. `AppRequest::GraphApplyPreset { dsl }`, `AppRequest::GraphJumpToPath(Vec<NoteId>)`). App routes them back to GraphTab via tab-id lookup. Clean separation but grows the AppRequest surface significantly.
2. **Move the action into the picker source**. The source holds a callback / state ref that fires on selection. Couples the picker to the host.
3. **Custom outcome enum per picker**. Each picker variant has its own outcome → tab dispatch. Breaks the uniform `Modal` interface.

The original design.md did not solve this. It needs a decision before the picker variants can migrate.

## 5. Map `input_mode` → `ActiveModal::QueryBar`

- [ ] 5.1 `/` key arm raises `OpenModal(ActiveModal::QueryBar { view_id: active })`
- [ ] 5.2 `Modal::handle_event` for `QueryBar` reads/writes the active view's query buffer (unchanged buffer location)
- [ ] 5.3 Esc returns `ModalOutcome::Closed`; Enter applies the query and returns `ModalOutcome::Closed`
- [ ] 5.4 Remove the `input_mode` flag from `GraphTab`; remove all `if !self.input_mode` guards in the tree-key arms

## 6. Status-bar modal indicator (preview)

- [ ] 6.1 Add a single status-bar cell that renders `App::active_modal_name()` when `Some`. Render an empty cell when `None`
- [ ] 6.2 Test: open each modal, assert the indicator cell renders the expected name

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
