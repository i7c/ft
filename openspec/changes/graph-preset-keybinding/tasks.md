## 1. State

- [ ] 1.1 Add `preset_picker_for_active_view: bool` field to `GraphTab` struct (default `false`)

## 2. Picker entry point

- [ ] 2.1 Add method `open_preset_picker_for_active_view(&mut self, ctx: &TabCtx)` that sets the flag to `true` and opens the picker (mirrors `add_view_with_presets` but does not create a new view)
- [ ] 2.2 Handle the "no presets" case: if the preset source is empty, return immediately without opening the picker

## 3. Picker continuation

- [ ] 3.1 In `handle_preset_picker_key`, check `preset_picker_for_active_view`; if `true`, call `apply_preset_to_active_view(&dsl)` on selection instead of the new-view path
- [ ] 3.2 Reset `preset_picker_for_active_view` to `false` when the picker closes (both on selection and on dismiss)

## 4. Keybinding

- [ ] 4.1 Add `(KeyCode::Char('p'), m) if m.contains(KeyModifiers::CONTROL)` arm in the main `handle_key` match that calls `open_preset_picker_for_active_view`

## 5. Help overlay

- [ ] 5.1 Add `("Ctrl+P", "load preset into this view")` to the "Query" section in `help_sections()`

## 6. Tests

- [ ] 6.1 Add a unit test in `graph.rs` verifying that `Ctrl+P` followed by a preset selection replaces the active view's query
- [ ] 6.2 Update the `TestBackend` snapshot in `ft/src/tui/tests.rs` if the help overlay output changes
- [ ] 6.3 Verify `cargo clippy --workspace --tests -- -D warnings` and `cargo fmt --check` pass
