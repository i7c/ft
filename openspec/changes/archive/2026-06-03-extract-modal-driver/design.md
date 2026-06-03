## Context

`GraphTab` (`ft/src/tui/tabs/graph.rs`) currently holds every modal-flow state as its own field on the struct (e.g. `create_state: Option<CreateState>`, `move_outer: Option<GraphMoveOuter>`, `preset_picker: Option<FuzzyPicker<PresetPickerSource>>`, plus 12 more). The event-handler is a long chain of `if self.<modal>.is_some() { return self.handle_<modal>_key(...); }` guards followed by the tab's own keymap. The `?` overlay collects per-tab help sections, but help for the *active modal* is improvised inside each modal handler.

This pattern works at small modal counts and breaks down at large ones. The fail modes are visible already:

- Adding a modal requires touching: a new field, the constructor, the dispatch chain (precedence implicit in ordering), the render branch, the help section, and a per-modal "close all other modals" coordination.
- The `input_mode` boolean (does the query bar own the keyboard?) is logically just another modal but lives outside the same pattern, causing every new modal to remember to gate against it.
- Cross-tab tests reach for private fields (`#[cfg(test)] fn selected_is_note_for_test`) because there's no uniform interface to ask "is a modal active right now."

The shared `notes_actions/` module already extracted *flow logic* (state-machine transitions, key handlers) but each tab still wraps it in its own outer state — `GraphMoveOuter` is the canonical example. The missing piece is a shared *outer driver*: one type that knows about every modal variant, dispatches keys, renders, and reports help.

## Goals / Non-Goals

**Goals:**

- One App-level slot owns the active modal. Tab-level modal slots disappear.
- Modal dispatch precedence is defined in one ordered match in `App`, not scattered across tab handlers.
- Modals expose a uniform interface: `handle_event`, `render`, `keymap_help` (used by the `?` overlay).
- TUI behaviour is byte-identical to the pre-change baseline. All snapshot tests pass unchanged (except mechanical struct-name updates).
- The `Modal` trait + `ActiveModal` enum are the foundation for the upcoming commands-and-keymaps change.

**Non-Goals:**

- No new modal types in this change.
- No keymap or UX changes — same chords, same precedence, same flows.
- No command/keymap layer (separate change).
- No migration of Tasks/Notes/Timeblocks/Journal tabs in this change — they keep their existing patterns until each tab grows enough to need the driver.
- No configurable bindings.
- No removal of `notes_actions/` — flows stay where they are; only the wrapping state moves.

## Decisions

### `ActiveModal` enum + `Modal` trait

```rust
// ft/src/tui/modal.rs
pub enum ActiveModal {
    Create(notes_actions::create::CreateState),
    Append(notes_actions::append::AppendState),
    CapturePicker(FuzzyPicker<CapturePresetPickerSource>),
    CaptureVar(notes_actions::capture::CaptureVarPromptState),
    MoveOuter(crate::tui::tabs::graph::GraphMoveOuter),  // tab-specific until migrated
    Rename(crate::tui::tabs::graph::GraphRenameState),
    PresetPicker(FuzzyPicker<PresetPickerSource>),
    Related(crate::tui::tabs::graph::RelatedModal),
    Search(FuzzyPicker<GraphSearchPickerSource>),
    PeriodicLeader,
    QueryBar { view_id: usize },
}

pub trait Modal {
    fn handle_event(&mut self, ev: Event, ctx: &mut TabCtx) -> ModalOutcome;
    fn render(&mut self, frame: &mut Frame, area: Rect, ctx: &TabCtx);
    fn keymap_help(&self) -> HelpSection;
    fn name(&self) -> &'static str;  // for status-bar indicator + tests
}

pub enum ModalOutcome {
    Consumed,                       // modal handled the key, still open
    Closed,                         // modal closed itself; App clears the slot
    OpenSibling(Box<ActiveModal>),  // current modal closed, new modal opens
    NotHandled,                     // key falls through to tab
}
```

Variants point at the existing state types, which now implement `Modal`. The enum lives at TUI top level; tab-specific variants reference tab modules.

**Alternative considered: trait object only.** `Option<Box<dyn Modal>>` instead of an enum. Rejected because the enum lets the App's status bar render a stable modal name (e.g., `MOVE: pick target`) by matching on variant — no virtual call into the modal. Snapshot diffability also benefits.

### Dispatch precedence centralised

`App::handle_event` becomes:

```rust
// 1. Modal first (if any).
if let Some(modal) = self.active_modal.borrow_mut().as_mut() {
    match modal.handle_event(ev, &mut ctx) {
        ModalOutcome::Consumed => return Ok(()),
        ModalOutcome::Closed => { *self.active_modal.borrow_mut() = None; return Ok(()); }
        ModalOutcome::OpenSibling(next) => { *self.active_modal.borrow_mut() = Some(*next); return Ok(()); }
        ModalOutcome::NotHandled => {}  // fall through
    }
}
// 2. Tab next.
let outcome = self.tabs[self.active].handle_event(ev, &mut ctx)?;
// 3. App-global keymap (Tab cycle, q to quit, ?).
self.handle_global_key(ev, outcome);
```

One match arm decides "modal first". Today this is replicated across every tab.

### Tabs request modals via `AppRequest::OpenModal`

Tabs no longer own modal state. They open a modal by setting `ctx.pending_request = Some(AppRequest::OpenModal(ActiveModal::Create(state)))`. The App services the request after `handle_event` returns (same flow as the existing `OpenInEditor` / `Toast` requests).

A tab can still consult `App::active_modal_name()` via the existing `TabCtx` if it needs to draw differently when a modal is up (e.g., dim the tree). Most don't need to.

### `Modal` for current state types

Each existing state type gets an `impl Modal`. The work is mechanical:

- `CreateState`: existing `handle_event` in `notes_actions/create.rs` → `Modal::handle_event` (signature already matches modulo wrapping).
- `AppendState`, `CaptureVarPromptState`, `SectionMoveState`: same.
- `FuzzyPicker<S>`: blanket `impl Modal for FuzzyPicker<S>` covers the three picker-flavour modals (preset, capture, search). The picker's existing `PickerOutcome` maps onto `ModalOutcome`.
- `GraphMoveOuter`, `GraphRenameState`, `RelatedModal`: tab-resident structs; the `Modal` impl lives in `tabs/graph.rs` for now and will move when the Graph tab's tab-specific modals are themselves promoted (future change).
- `PeriodicLeader` is a unit variant; its `handle_event` is a single match on the leader letter — stays tiny.

### `input_mode` becomes `ActiveModal::QueryBar`

The query bar is conceptually a modal: it captures all printable characters and `Enter` runs a query. Promoting it to a variant lets dispatch precedence handle it identically to other modals, removes the special-case boolean, and gives the status bar a name to display.

### Snapshot baseline

`ft/src/tui/tests.rs` has ~293 tests. The migration must produce no snapshot diffs. Two safety nets:

1. CI invariant: snapshot diff = regression. Authors must justify each diff and re-bless deliberately.
2. The `ft/src/tui/tabs/graph.rs` tests (43 of them) are the most exposed; we run them with `INSTA_FORCE_PASS=0` before and after the migration and verify identical artifacts.

### Help dispatch

When a modal is active, `?` opens the modal's `keymap_help()` overlay, not the tab's. The tab help is reachable by closing the modal first. The shared global section (Tab cycle, quit, `?`) is always appended.

## Risks / Trade-offs

- **[Dispatch-precedence change is the risky part]** → A long chain of `if self.<modal>.is_some() { ... }` arms encodes precedence implicitly by ordering. The new code orders modal-before-tab once; tab-internal precedence between, e.g., the Graph tab's `move_outer` and `rename_state` becomes the order of variants in the enum's match arms. Audit: list the existing precedence chain (graph.rs), map each entry to an `ActiveModal` variant, verify the new match preserves the same order.
- **[Tab-specific modal variants still live in tab modules]** → `GraphMoveOuter` / `GraphRenameState` / `RelatedModal` reference `tabs/graph.rs` types. This is intentional — the abstraction is "App owns the slot," not "every modal lives in `modal.rs`." A future change can promote them once the Graph tab's own logic is split.
- **[`AppRequest::OpenModal` carries an enum with heap-ish variants]** → The enum is large (~200 bytes worst case). `AppRequest` already has `#[allow(clippy::large_enum_variant)]` precedent at the App boundary; same approach.
- **[Tab tests that probe modal state now route through App]** → `selected_is_note_for_test` and the cross-tab Journal-jump test reach into private fields. Replace with `App::active_modal_name()` accessors for tests. This is an improvement; documented in tasks.

## Open Questions

- Should `ActiveModal::QueryBar` carry the buffer / cursor state, or is the query bar state still on the view? **Leaning:** state stays on the view; the variant just signals "this view's query bar has the keyboard." Avoids moving 80 lines of view-local state.
- Future migration order for Tasks/Notes/Timeblocks tabs? Out of scope — each is its own change when needed.
