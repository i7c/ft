## Why

`ft/src/tui/tabs/graph.rs` has become a 4,276-line god-file because every new note-touching action accumulates a modal slot on `GraphTab`. The struct holds ~15 modal slots (`create_state`, `append_state`, `capture_picker`, `capture_var_state`, `periodic_leader`, `move_outer`, `rename_state`, `preset_picker`, `related_modal`, `search_picker`, …) plus an `input_mode` flag, and every new flow has to re-derive the same dispatch-precedence logic. The next 1–2 features will push the file past 5k lines and the arbitration logic past the point where it's reviewable.

The shared `notes_actions/` module was the right refactor for flow *internals*; what's missing is a shared *driver* — one place that owns "which modal is active, who gets the keyboard, what renders on top." This change extracts that driver and migrates the Graph tab to it. Subsequent changes can migrate the other tabs at their own pace.

## What Changes

- Introduce `ActiveModal` (enum) and a `Modal` trait on `ft/src/tui/modal.rs`. Each existing modal type implements `Modal::handle_event`, `Modal::render`, `Modal::keymap_help`.
- Move the `ActiveModal` slot off `GraphTab` and onto `App`. The slot is `RefCell<Option<ActiveModal>>` so existing borrow patterns keep working.
- Update the App event loop's dispatch order: `ActiveModal::handle_event` runs ahead of `Tab::handle_event`; tabs request a modal via a new `AppRequest::OpenModal(ActiveModal)` variant.
- Migrate every modal currently held on `GraphTab` (create / append / capture / capture-var / move-outer / rename / preset-picker / related-modal / search-picker / periodic-leader) into the `ActiveModal` enum. The `input_mode` query-bar flag becomes `ActiveModal::QueryBar { view_id }`.
- The other tabs (Tasks / Notes / Timeblocks / Journal) keep their existing modal patterns *in this change*. Their migration is the same recipe — separate openspec changes when those tabs grow.
- TUI behaviour is snapshot-identical. The 43 graph-tab tests and the dozens of cross-tab tests in `ft/src/tui/tests.rs` must pass without snapshot diffs (except mechanical name changes).

## Capabilities

### New Capabilities

- `tui-modal-driver`: A single App-level slot owns the currently active modal; modals implement a uniform `Modal` trait covering key dispatch, rendering, and help-section reporting. Modal dispatch precedence is centralised in one ordered match.

### Modified Capabilities

<!-- None in this change. Tab keymaps, query DSLs, and CLI surfaces are untouched. -->

## Impact

- **New**: `ft/src/tui/modal.rs` (≈ 200 lines: `Modal` trait, `ActiveModal` enum, `ModalDispatch` helper).
- **`ft/src/tui/app.rs`**: new `active_modal: RefCell<Option<ActiveModal>>` slot, new `handle_modal_event` step in the event loop, new `service_open_modal_request` for `AppRequest::OpenModal`.
- **`ft/src/tui/tab.rs`**: new `AppRequest::OpenModal(ActiveModal)` variant. `Tab` trait gains `default_keymap_help()` (modal help replaces tab help when a modal is active).
- **`ft/src/tui/tabs/graph.rs`**: drops ~15 fields and ~12 `if self.<modal>.is_some()` dispatch branches; gains modal-launch calls (`ctx.request(AppRequest::OpenModal(...))`) on the `c`/`C`/`A`/`Q`/`m`/`r`/`R`/`f`/`Ctrl+N`/`Ctrl+P` key arms. Net line reduction expected ≈ −1,200.
- **`ft/src/tui/notes_actions/`**: each flow's `State` types are exposed via the `Modal` trait — no behavioural changes.
- **Tests**: snapshot-baseline gate. Existing snapshots in `ft/src/tui/tests.rs` must not change. New unit tests cover dispatch precedence and the round-trip "open modal → handle key → close modal" through `App`.
- All four build invariants (`build --release`, `test --workspace`, `clippy -D warnings`, `fmt --check`) stay green.
