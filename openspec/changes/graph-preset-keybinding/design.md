## Context

The graph tab (`ft/src/tui/tabs/graph.rs`) already has a complete preset subsystem:

- `preset_picker: Option<FuzzyPicker<PresetPickerSource>>` — modal fuzzy-picker widget reused for `Ctrl+N`.
- `add_view_with_presets` — opens the picker and, on selection, creates a **new** view pre-filled with the preset DSL.
- `apply_preset_to_active_view(&dsl)` — applies a DSL string to the currently active view in-place.
- `resolve_preset(name, ctx)` — resolves a preset name to its DSL (user config → built-in).
- `handle_preset_picker_key` — routes picker keyboard events and calls `add_view_with_presets` on confirmation.

The only missing piece is a second entry point that opens the picker but, on selection, calls `apply_preset_to_active_view` instead of adding a new view. The picker widget itself does not need to change; only the *continuation* after selection differs.

## Goals / Non-Goals

**Goals:**
- `Ctrl+P` on the graph tab opens the fuzzy preset picker bound to the *active* view.
- On selection, the active view's query is replaced with the preset DSL and the graph re-runs.
- On dismiss (Esc), nothing changes.
- The binding appears in `help_sections()` under "Query".

**Non-Goals:**
- No changes to the picker widget itself.
- No new preset storage or resolution logic.
- No changes to the `Ctrl+N` flow.

## Decisions

### Distinguish picker intent without a second picker type

**Decision:** Add a boolean flag `preset_picker_for_active_view: bool` on `GraphTab` that is set to `true` when opening via `Ctrl+P` and `false` (the default, current behaviour) when opening via `Ctrl+N`.

`handle_preset_picker_key` reads this flag to decide whether to call `apply_preset_to_active_view` or `add_view_with_presets_blank`.

**Alternatives considered:**
- *Enum instead of bool* — `PickerTarget { ActiveView, NewView }` would be clearer at a larger call site, but for a two-state discriminant it adds ceremony without benefit.
- *Two separate picker fields* — avoids the flag but means only one picker can be open at a time anyway (they're modal), so the extra field is pure complexity.

## Risks / Trade-offs

- [Key conflict] `Ctrl+P` is currently unused in the TUI — verified by grep. Low risk.
- [Picker state confusion] If the flag gets out of sync (e.g., opened as `Ctrl+N` but flag says active-view), the wrong continuation runs. Mitigation: always set the flag immediately before `self.preset_picker = Some(...)` in the same method, never in a separate step.
