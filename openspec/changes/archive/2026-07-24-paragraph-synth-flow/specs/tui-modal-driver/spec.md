## ADDED Requirements

### Requirement: `ParagraphSynth` modal variant
The paragraph-synth flow's state SHALL be wrapped in a new `ActiveModal::ParagraphSynth(ParagraphSynthState)` variant and managed by the App's `active_modal` slot. The flow's step state machine (source pick → paragraph multi-select → target pick → commit) SHALL live in `ft/src/tui/notes_actions/paragraph_synth.rs`, mirroring `section_move.rs`'s shape (state enum + free-function key handlers returning a step outcome), with `Modal::handle_event` wrapping the handler and `Modal::render` rendering the active step. No tab SHALL hold a paragraph-synth state field; all entry is via `AppRequest::OpenModal(ActiveModal::ParagraphSynth(...))`.

#### Scenario: `y` on the Graph tab posts OpenModal
- **WHEN** the user presses `y` on the Graph tab with a Note node focused and a clean tree
- **THEN** the tab posts `AppRequest::OpenModal(Box::new(ActiveModal::ParagraphSynth(state)))` seeded into the paragraph-multi-select step and returns `EventOutcome::Consumed`

#### Scenario: Dispatch routes through the modal driver
- **WHEN** `ActiveModal::ParagraphSynth(...)` is the active modal
- **THEN** App's modal-first dispatch routes key events to the modal's `handle_event`; the underlying tab's `handle_event` is not called for those keys

#### Scenario: Status-bar indicator shows the modal name
- **WHEN** any step of `ParagraphSynth` is the active modal
- **THEN** `App::active_modal_name()` returns a stable name and the status bar's right cell renders `modal: <name>` in magenta

#### Scenario: Graph tab tree-seeded entry skips the source picker
- **WHEN** the Graph tab opens `ParagraphSynth` with a focused Note node
- **THEN** the modal's initial state is the paragraph-multi-select step seeded to the focused note (no source-picker step), mirroring the `GraphMoveOuter::SourceFromTree` tree-seeded entry pattern
