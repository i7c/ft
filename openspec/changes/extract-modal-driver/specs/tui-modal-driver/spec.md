## ADDED Requirements

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

All TUI snapshot tests in `ft/src/tui/tests.rs` and any per-module snapshot tests SHALL pass after the migration. The only allowed snapshot diff is the right-cell status-bar text changing from `mode: <label>` to `modal: <name>` when a modal is active — this is itself a specified requirement of §6 (the modal indicator) and is re-blessed deliberately per migration commit. No other behavior may change.

#### Scenario: Graph tab tests pass with only the §6 status-bar diff
- **WHEN** the Graph tab snapshot tests are run after migration
- **THEN** every snapshot matches its committed baseline byte-for-byte EXCEPT for the right-cell status-bar text on tests that exercise an active modal (which renders `modal: <name>` per §6)

#### Scenario: Cross-tab tests pass unchanged
- **WHEN** the cross-tab tests (e.g., Graph → Journal jump) are run after migration
- **THEN** every assertion holds and every snapshot matches its baseline (cross-tab navigation does not involve modal state)

### Requirement: GraphMoveOuter is explicitly out of scope

The 7-variant `GraphMoveOuter` state machine SHALL remain a tab-resident `Option<GraphMoveOuter>` field on `GraphTab` after this change, with its existing dispatch and render path intact. The `ActiveModal::MoveOuter` enum variant and stub `Modal` impl exist so the enum is closed and the trait dispatch is total; no `OpenModal` call in this change wraps `MoveOuter`. Migrating it follows the patterns established by this change and is the subject of a separate follow-up.

#### Scenario: MoveOuter dispatch is unchanged
- **WHEN** the user presses `m` to start a move-section flow on the Graph tab
- **THEN** the existing `move_outer = Some(GraphMoveOuter::SourceFromTree)` path runs (no `OpenModal` posted) and the rest of the flow operates through the legacy `if self.move_outer.is_some()` dispatch arm and render arm

#### Scenario: MoveOuter does not surface a modal-indicator
- **WHEN** any `GraphMoveOuter` variant is active
- **THEN** the right-cell status bar continues to show `mode: <label>` (no `modal: move` indicator), because `App::active_modal` is `None` during a move flow
