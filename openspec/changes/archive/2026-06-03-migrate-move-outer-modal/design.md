## Context

The `GraphMoveOuter` enum has 7 variants representing phases of the
graph-tab move-section flow:

- `SourceFromTree` — `m` pressed; awaiting confirm-source/picker/Esc
- `SourcePicker { picker }` — `t` opens a fuzzy source picker
- `Inner(SectionMoveState)` — wraps the shared notes-tab move flow
- `TargetFromTree { carry }` — tree-driven target picking
- `TargetPicker { picker, carry }` — fuzzy target picker
- `MoveTargetFromTree { selected }` — Flow A (Space-select + r) target dir
- `MoveTargetPicker { picker, selected }` — Flow A fuzzy target dir

The flow's handler (`handle_move_key`, ~217 LoC) dispatches on the
variant and either transitions the outer state, advances the inner
`SectionMoveState`, posts a toast, or commits. The render arm
(~80 LoC) draws either a status banner over the tab strip
(tree-driven phases) or delegates to `notes_view::render_move_overlay`
via a `SectionMoveState` wrapper trick (picker phases). The flow
calls three GraphTab helper methods that read/mutate graph + view
state: `selected_note_hit`, `confirm_target_from_tree`,
`confirm_move_target`, and `apply_inner_step`.

The prior `extract-modal-driver` change established the pattern for
tab-resident state migrations (`GraphRenameState`, `RelatedModal`):
state stays in `tabs/graph.rs`, `Modal` impl lives there too, and
state-touching commits route back via tab-specific `AppRequest`
variants. This change applies that recipe to `GraphMoveOuter`.

## Goals / Non-Goals

**Goals:**

- `GraphMoveOuter` flows through `ActiveModal` like every other modal.
- The `modal: move` status-bar indicator displays during move flows.
- All existing move-flow tests pass with at most deliberate status-bar diffs.
- Logic stays in `tabs/graph.rs` — picker newtypes and `Modal` impl
  next to the `GraphMoveOuter` definition.
- Internal pickers use `ModalOutcome::OpenSibling` to transition
  between sub-phases (first wide use of `OpenSibling` for a multi-step
  modal — `CapturePicker → CaptureVar` is the existing one-shot
  precedent).

**Non-Goals:**

- No UX changes. Same keymap, same flow shape, same renders.
- No refactor of `notes_actions::section_move` (the `SectionMoveState`
  used by `MoveOuter::Inner` already has a `Modal` impl that's
  unchanged here).
- No new test scenarios — re-use existing move-flow integration tests
  with re-blessed status-bar snapshots only.

## Decisions

### `GraphMoveOuter::handle_event` mirrors `handle_move_key`

```rust
impl Modal for GraphMoveOuter {
    fn handle_event(&mut self, ev: Event, ctx: &TabCtx) -> ModalOutcome {
        let Event::Key(k) = ev else { return NotHandled };
        match self {
            Self::SourceFromTree => match (k.code, k.modifiers) {
                (Char('m'), NONE) => {
                    ctx.pending_request = Some(GraphMoveConfirmSourceFromTree);
                    Closed
                }
                (Char('t'), NONE) => OpenSibling(Box::new(
                    MoveOuter(SourcePicker { picker: FuzzyPicker::new(...) })
                )),
                (Esc, _) => Closed,
                _ => Consumed,
            },
            Self::SourcePicker { picker } => match picker.handle_key(k) {
                Selected(hit) => OpenSibling(Box::new(
                    MoveOuter(Inner(advance_to_multiselect(hit)))
                )),
                Cancelled => OpenSibling(Box::new(MoveOuter(SourceFromTree))),
                ...
            },
            Self::Inner(inner) => /* delegate to inner.handle_event,
                                    intercept TargetPicking transition */,
            Self::TargetFromTree { .. } => /* similar pattern */,
            ...
        }
    }
}
```

The handler is mostly verbatim from `handle_move_key`. The
difference: transitions become `OpenSibling(Box<ActiveModal>)`
instead of `self.move_outer = Some(NextVariant)`. Commits become
`AppRequest::GraphMove*` posts instead of direct method calls.

### Internal pickers as newtypes (optional)

The three internal pickers (`SourcePicker`, `TargetPicker`,
`MoveTargetPicker`) are currently inline `FuzzyPicker<VaultFilePickerSource>`
fields on variants. They could stay inline OR be promoted to newtypes
like `SearchPickerModal`. **Decision: keep inline.** They're already
contained within the outer modal; promoting them would add an extra
layer of `ActiveModal` variants for state the outer modal already
manages. The handler matches on the outer variant and dispatches to
the inner picker's `handle_key` directly.

### State-touching commits route via AppRequest

Three commit actions can't run inside the modal because they need
read access to `self.graph` and write access to the host's view
state:

- **Confirm source from tree** (`m` in `SourceFromTree`):
  Calls `selected_note_hit()` to get the selected note path, then
  builds the `Inner(HeadingMultiSelect)` state via
  `advance_to_multiselect`. The modal can't reach the graph.
  → `AppRequest::GraphMoveConfirmSourceFromTree`. Host runs the
  logic, posts `OpenModal(MoveOuter(Inner(state)))` on success.
- **Confirm target from tree** (`m` in `TargetFromTree`):
  `selected_note_hit` + `compose_with_existing_target` + write
  target file. Same pattern.
  → `AppRequest::GraphMoveConfirmTargetFromTree`.
- **Flow A confirm move target** (`m` or Enter in `MoveTargetFromTree`):
  Reads selected directory, plans + applies multi-rename, refreshes
  graph. Same pattern.
  → `AppRequest::GraphMoveConfirmMoveTarget`.

### Inner step intercept stays in the host

The `Inner(SectionMoveState)` variant delegates to
`SectionMoveState::handle_event` (existing Modal impl). When the
inner returns a `Transition(TargetPicking { … })`, the Graph tab
intercepts and reroutes to `TargetFromTree` (per the existing
`apply_inner_step` helper). With the migration, the modal's
`handle_event` for `Inner` calls `inner.handle_event`, then if the
inner posted any AppRequest indicating a `TargetPicking` transition,
post `OpenSibling(MoveOuter(TargetFromTree { carry }))` on top of it.

In practice: the inner's `handle_event` doesn't know about the outer
wrapping. The simplest mechanism is to keep `apply_inner_step` on
GraphTab and have it called via a hook (e.g.
`AppRequest::GraphMoveApplyInnerStep(SectionMoveState transition)`) —
but that's clunky.

**Cleaner approach:** keep the `apply_inner_step` intercept logic
inline in `GraphMoveOuter::handle_event` for `Inner(...)`. The modal
itself decides "if inner returned the TargetPicking transition,
swap to TargetFromTree." This is doable because `MoveStep::Transition`
carries the next `SectionMoveState` by value, and the modal can
match on its variant.

### Render lift

The render arm currently uses a trick: for `SourcePicker` and
`TargetPicker`, it wraps the picker in a throwaway
`SectionMoveState::SourcePicking { picker }` / `TargetPicking { ... }`
and calls `notes_view::render_move_overlay` so the existing renderer
handles the chrome. The trick uses `std::mem::replace` to swap the
picker out and back.

**Decision: keep the trick verbatim.** It's the cleanest way to reuse
the shared renderer. The lifted `Modal::render` just executes the
same trick — the trick lives inline, no new abstraction needed.

For `SourceFromTree`, `TargetFromTree`, and `MoveTargetFromTree`,
the render is a one-line status banner via `render_move_banner` (a
free helper in `tabs/graph.rs`). Lifted directly.

For `MoveTargetPicker`, the render is just `picker.render(frame, area)`
(no chrome). Lifted directly.

### State-mutating actions take ctx

The `Tab::graph_move_*` hooks all take `ctx: &TabCtx` (mirrors
`graph_commit_rename` from the prior change) so they can read
`ctx.vault` and post follow-up `AppRequest`s.

## Risks / Trade-offs

- **[Handler is large]** → 217 LoC of lift into one `Modal` impl. Big
  diff but mechanical. Mitigation: split the handler into
  per-variant private methods on `GraphMoveOuter` (like `handle_source_from_tree(k, ctx)`, etc.) called from the
  top-level `match self`. Keeps each branch ~30 LoC.
- **[Picker outcomes vs OpenSibling]** → Internal pickers' `Selected`
  outcomes transition to a new outer variant. `OpenSibling(Box<MoveOuter(NextVariant)>)`
  is the natural shape but means the modal driver swaps the slot on
  every internal step. Slightly wasteful (re-allocates the boxed
  `ActiveModal`) but consistent with `OpenSibling`'s intended use.
- **[`Inner` intercept is subtle]** → The shared `SectionMoveState`
  doesn't know about the graph-tab outer wrapper; intercepting the
  `TargetPicking` transition requires the modal to inspect the
  inner's return value. Acceptable — the existing `apply_inner_step`
  already does this. The migration just moves where the inspection
  happens.
- **[Status-bar shows `modal: move` during what users may think of
  as a "tab state"]** → This is actually a UX improvement: users
  in mid-move flow get clear feedback that they're in a modal
  context.

## Open Questions

- Should the modal name be `move`, `move-section`, or `graph-move`?
  Leaning: `move` (short, fits the 16-char status-bar cell, matches
  the existing `name()` of the stub impl).
- Should `apply_inner_step` move into the modal or stay on GraphTab?
  Leaning: stay on GraphTab. It mutates view state via
  `restore_all_views()` and is one of the helpers commit hooks
  call. Keeps logic with the graph it operates on.
