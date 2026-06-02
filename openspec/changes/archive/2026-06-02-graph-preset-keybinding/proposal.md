## Why

The graph tab can already load presets when opening a **new** view (`Ctrl+N`), but there is no way to apply a preset to the **current** active view without closing and re-opening it. Users who want to switch the active view's query to a named preset must type the DSL manually.

## What Changes

- Add `Ctrl+P` keybinding on the graph tab that opens the preset picker and applies the selected preset DSL to the **currently active view** (replacing its query in-place).
- Wire the binding into `help_sections()` under the "Query" group so it appears in the `?` overlay.

## Capabilities

### New Capabilities

- `graph-load-preset`: Keybinding (`Ctrl+P`) that opens the fuzzy preset picker on the active graph view and applies the chosen preset's DSL to that view's query.

### Modified Capabilities

<!-- No existing spec-level requirements are changing. -->

## Impact

- `ft/src/tui/tabs/graph.rs`: new key arm in the main `handle_key` match, new method `open_preset_picker_for_active_view`, update to `help_sections()`.
- No changes to `ft-core`; the preset resolution logic (`resolve_preset`) already exists and can be reused.
- No breaking changes.
