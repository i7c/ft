## ADDED Requirements

### Requirement: `CompletionProvider` trait and `CompletionItem` value type

A `CompletionProvider` trait SHALL be defined with a single core method `fn complete(&mut self, ctx: &CompletionContext) -> Vec<CompletionItem>` plus `fn trigger_on(&self) -> TriggerSet`. A `CompletionItem` SHALL carry at minimum: `label`, `insert_text`, `replace_span`, `kind`, optional `description`.

#### Scenario: Provider is queried with current buffer state
- **WHEN** an `EditBuffer` with an attached provider receives an input event matching `trigger_on()`
- **THEN** the provider is called with a `CompletionContext` whose `text` reflects the buffer's current text and `cursor_byte` reflects the current cursor position

#### Scenario: Provider returns items, popup opens
- **WHEN** the provider returns a non-empty `Vec<CompletionItem>` for the current context
- **THEN** the `CompletionPopup` opens with those items in the returned order; the popup does NOT re-rank

#### Scenario: Provider returns items with replace_span
- **WHEN** an item's `replace_span` is `Some(range)` and the user accepts it
- **THEN** the buffer replaces the byte range `range` with `insert_text` and places the cursor immediately after the inserted text

#### Scenario: Provider returns items without replace_span
- **WHEN** an item's `replace_span` is `None` and the user accepts it
- **THEN** the buffer replaces the current word (the boundary-delimited token containing the cursor) with `insert_text`

### Requirement: `CompletionPopup` widget renders at cursor position

The `CompletionPopup` widget SHALL render as a vertical list of items near the host edit buffer's cursor. The popup SHALL position itself below the cursor when the cursor is in the upper half of the host area, and above the cursor when in the lower half. The popup SHALL be clamped to the screen bounds.

#### Scenario: Popup renders below cursor in upper area
- **WHEN** the cursor is at row 5 of a 20-row area
- **THEN** the popup renders at rows 6 through 6+N (clamped to the area's bottom)

#### Scenario: Popup renders above cursor in lower area
- **WHEN** the cursor is at row 15 of a 20-row area
- **THEN** the popup renders at rows 15-N through 14 (clamped to the area's top)

#### Scenario: Popup is dismissed by Esc
- **WHEN** the popup is open and the user presses Esc
- **THEN** the popup closes, the buffer is unchanged, and Esc is consumed (does not close the host modal)

### Requirement: Completion popup participates in modal dispatch precedence

The `CompletionPopup` SHALL be dispatched ahead of the host modal's other key handling. A key consumed by the popup SHALL NOT reach the host modal; a key the popup does not handle SHALL fall through to the host modal's `dispatch_command`.

#### Scenario: Popup consumes navigation keys
- **WHEN** the popup is open and the user presses `Down`
- **THEN** the popup moves its selection one item down; the host modal does not see the `Down` key

#### Scenario: Popup consumes Tab / Enter for accept
- **WHEN** the popup is open and the user presses `Tab` or `Enter`
- **THEN** the popup inserts the selected item, closes, and the host modal does not see the key

#### Scenario: Printable characters fall through to host modal
- **WHEN** the popup is open and the user types a printable character
- **THEN** the host modal's `dispatch_command` (or the buffer's char-insert) handles the character, then the provider is re-queried with the new buffer state and the popup updates

### Requirement: Scaffold ships with no concrete providers

This change SHALL NOT ship any concrete `CompletionProvider` implementation other than a `StubCompletionProvider` used by tests. Concrete providers (graph DSL, file paths, tags) are out of scope and are tracked as follow-up changes.

#### Scenario: No real provider is mounted by default
- **WHEN** the TUI starts and the user opens any text-input modal (query bar, picker, rename, quickline)
- **THEN** the `EditBuffer.completion` slot is `None` and no popup ever opens
