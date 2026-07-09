## ADDED Requirements

### Requirement: Task preset picker is an `ActiveModal` variant

The Tasks-tab preset picker SHALL be expressed as a new `ActiveModal::TaskPresetPicker` variant — a `Modal` wrapper around `FuzzyPicker<TaskPresetPickerSource>` — installed via `AppRequest::OpenModal(...)` and managed by the App's single `active_modal` slot. No tab-level `Option<...>` field SHALL hold picker state. On `PickerOutcome::Selected(name)`, the modal SHALL resolve the preset name to its DSL string (user `Config::presets` first, then `query::preset::builtin`), post `AppRequest::Tasks(TasksRequest::ApplyPreset(dsl))`, and return `ModalOutcome::Closed`. On `PickerOutcome::Cancelled`, it SHALL return `ModalOutcome::Closed` with no request posted (the active view's query is unchanged). `PickerOutcome::StillOpen` and `NotHandled` map to `Consumed` and `NotHandled` respectively, matching the existing picker-modal pattern.

The picker's `Modal::name()` SHALL be `"task-preset-picker"` (stable for the status-bar modal indicator and tests), and its `commands()` / `keymap()` SHALL return the `modal/task-preset-picker`-scoped registry entries so the `?` overlay, `ft commands list`, and user keymap overrides see it.

#### Scenario: Tasks tab opens the picker via OpenModal
- **WHEN** the Tasks tab handles `Ctrl+P` (no other modal active, not in a sub-mode)
- **THEN** it sets `pending_request = Some(AppRequest::OpenModal(ActiveModal::TaskPresetPicker(...)))` and the App services the request after `handle_event` returns

#### Scenario: Modal commits via the Tasks request channel
- **WHEN** the tasks preset picker has a selected preset and the user presses Enter
- **THEN** the modal posts `AppRequest::Tasks(TasksRequest::ApplyPreset(dsl))` and returns `ModalOutcome::Closed`; the App's modal-first dispatch then clears the slot

#### Scenario: Modal cancel does not touch the active query
- **WHEN** the tasks preset picker is open and the user presses Esc
- **THEN** the modal returns `ModalOutcome::Closed` without posting any `AppRequest`, and the active SearchView's query text is unchanged

#### Scenario: Modal name surfaces in the status bar
- **WHEN** the tasks preset picker is the active modal
- **THEN** `App::active_modal_name()` returns `Some("task-preset-picker")` and the status-bar modal indicator renders that name
