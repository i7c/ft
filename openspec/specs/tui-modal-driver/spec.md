# tui-modal-driver

## Purpose

Provide a single App-level modal slot and a uniform `Modal` interface so every
keyboard-capturing overlay in the TUI — pickers, multi-step flows (create /
append / move-section), inline modals (rename, related, query-bar), chord
leaders — is dispatched, rendered, documented, and identified through the
same code path. Tabs install modals by raising `AppRequest::OpenModal(...)`
(or the combined `OpenModalWithToast`); modals close, swap, or fall through
via `ModalOutcome`. No tab holds private modal-slot fields.

## Requirements

### Requirement: A single App-level slot owns the currently active modal

The App SHALL hold exactly one `Option<ActiveModal>` slot. When a modal is open, that slot is `Some`; otherwise it is `None`. No tab SHALL hold its own modal-state field for any modal expressible as an `ActiveModal` variant.

#### Scenario: No modal active
- **WHEN** the user is navigating a tab with no overlay open
- **THEN** `App::active_modal()` returns `None` and the tab's `handle_event` receives every key

#### Scenario: One modal at a time
- **WHEN** the user opens the create-note flow from the Graph tab
- **THEN** `App::active_modal()` returns `Some(ActiveModal::Create(_))` and no other modal can be in flight

#### Scenario: Tab opens a modal via AppRequest
- **WHEN** the Graph tab handles a `c` key and wants to open the create flow
- **THEN** it sets `pending_request = Some(AppRequest::OpenModal(ActiveModal::Create(state)))` and the App services the request after `handle_event` returns

### Requirement: Modal dispatch runs ahead of tab dispatch

The App event loop SHALL dispatch incoming events to the active modal before the active tab. A modal that consumes an event SHALL prevent the tab from seeing it; a modal that returns `NotHandled` SHALL let the event fall through to the tab.

#### Scenario: Modal consumes
- **WHEN** a modal is active and the user presses any key the modal's `handle_event` returns `ModalOutcome::Consumed` for
- **THEN** the active tab's `handle_event` is not called for that key

#### Scenario: Modal closes itself
- **WHEN** a modal's `handle_event` returns `ModalOutcome::Closed` (e.g., the user presses Esc)
- **THEN** the App sets `active_modal = None` and the next key reaches the tab

#### Scenario: Modal opens a sibling
- **WHEN** a modal's `handle_event` returns `ModalOutcome::OpenSibling(next)` (e.g., section-move advances from source-picker to heading-multiselect)
- **THEN** the App replaces the slot with `next` and the new modal owns the next key

#### Scenario: Modal lets a key pass through
- **WHEN** a modal returns `ModalOutcome::NotHandled` for a key
- **THEN** the App routes that same event to the active tab's `handle_event`

### Requirement: Modals expose a uniform interface

Every `ActiveModal` variant SHALL implement `Modal::handle_event`, `Modal::render`, `Modal::keymap_help`, and `Modal::name`. These four methods are the entire interface the App needs to dispatch, draw, document, and identify a modal.

#### Scenario: Active modal renders on top
- **WHEN** a modal is active and the App draws a frame
- **THEN** the tab's `render` is called first, then the active modal's `render` draws over the same area

#### Scenario: Help overlay shows modal sections when a modal is active
- **WHEN** the user presses `?` while a modal is active
- **THEN** the help overlay shows the active modal's `keymap_help()` followed by the global section, NOT the tab's `help_sections()`

#### Scenario: Modal name is stable for status bar and tests
- **WHEN** the search picker is active
- **THEN** `App::active_modal().unwrap().name() == "search"`, and the same string is rendered in the status-bar modal indicator and asserted in tests

### Requirement: The query-bar input mode is a modal variant

The Graph tab's query-bar focus (previously the `input_mode` boolean) SHALL be expressed as `ActiveModal::QueryBar { view_id }`. The boolean field SHALL be removed.

#### Scenario: Slash key opens query bar
- **WHEN** the user presses `/` on the Graph tab with no other modal active
- **THEN** the tab raises `AppRequest::OpenModal(ActiveModal::QueryBar { view_id: <active> })`

#### Scenario: Esc closes query bar
- **WHEN** the query bar is active and the user presses Esc
- **THEN** the modal returns `ModalOutcome::Closed` and the App clears the slot

#### Scenario: Tab keys outside the query bar still work
- **WHEN** no modal is active and the user presses `j` / `k` / `l` / `h`
- **THEN** the keys reach the tab and operate on the tree (unchanged from baseline)

### Requirement: Snapshot baseline preserved with one deliberate diff

All TUI snapshot tests in `ft/src/tui/tests.rs` and any per-module snapshot tests SHALL pass after the migration. The only allowed snapshot diff is the right-cell status-bar text changing from `mode: <label>` to `modal: <name>` when a modal is active — this is itself a specified requirement of the modal indicator and is re-blessed deliberately per migration commit. No other behavior may change.

#### Scenario: Graph tab tests pass with only the status-bar diff
- **WHEN** the Graph tab snapshot tests are run after migration
- **THEN** every snapshot matches its committed baseline byte-for-byte EXCEPT for the right-cell status-bar text on tests that exercise an active modal (which renders `modal: <name>`)

#### Scenario: Cross-tab tests pass unchanged
- **WHEN** the cross-tab tests (e.g., Graph → Journal jump) are run after migration
- **THEN** every assertion holds and every snapshot matches its baseline (cross-tab navigation does not involve modal state)

### Requirement: `GraphMoveOuter` flows through `ActiveModal`

The `GraphMoveOuter` state SHALL be wrapped in `ActiveModal::MoveOuter(GraphMoveOuter)` and managed by the App's `active_modal` slot. The `move_outer: Option<GraphMoveOuter>` field on `GraphTab` SHALL NOT exist. The `if self.move_outer.is_some()` dispatch arm in `GraphTab::handle_event` SHALL NOT exist. The variant-specific render arm in `GraphTab::render` SHALL NOT exist.

#### Scenario: `m` posts OpenModal
- **WHEN** the user presses `m` on the Graph tab with no other modal active
- **THEN** the tab posts `AppRequest::OpenModal(Box::new(ActiveModal::MoveOuter(GraphMoveOuter::SourceFromTree)))` and returns `EventOutcome::Consumed`

#### Scenario: Dispatch routes through the modal driver
- **WHEN** `ActiveModal::MoveOuter(...)` is the active modal
- **THEN** App's modal-first dispatch routes key events to `GraphMoveOuter::handle_event`; the tab's `handle_event` is not called for those keys

#### Scenario: Status-bar indicator shows `modal: move`
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
