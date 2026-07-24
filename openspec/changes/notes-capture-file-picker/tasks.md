## 1. Capture flow: `NeedsTarget` outcome

- [ ] 1.1 Add `CaptureTargetPromptState` struct to `ft/src/tui/notes_actions/capture.rs` holding `template_source`, `section_override`, `vars_needed`, and `picker: FuzzyPicker<VaultFilePickerSource>`.
- [ ] 1.2 Add `CaptureResult::NeedsTarget(CaptureTargetPromptState)` variant.
- [ ] 1.3 Change `try_execute_preset`'s append arm: when `preset.note` is `None` and `target_note_override` is `None`, return `Ok(CaptureResult::NeedsTarget(...))` instead of `Err("no target note for append preset")`. Keep the `target_path.exists()` check deferred to after the picker resolves a path.
- [ ] 1.4 Add a helper that takes a resolved `PathBuf` + the prompt state's fields and either commits immediately (no vars) or builds a `CaptureVarPromptState` (vars present), returning `CaptureResult`. Reuse existing `commit_capture` / `CaptureCommit`.

## 2. Notes tab: file-picker state

- [ ] 2.1 Add `NotesState::CaptureFilePicking(CaptureTargetPromptState)` variant in `ft/src/tui/tabs/notes/mod.rs`.
- [ ] 2.2 In `handle_capture_picker_key`, handle `CaptureResult::NeedsTarget(state)` by transitioning to `NotesState::CaptureFilePicking(state)` (instead of the current `Err` toast path).
- [ ] 2.3 Add `handle_capture_file_picker_key`: on `PickerOutcome::Selected(hit)`, resolve `ctx.vault.path.join(&hit.path)`, run the helper from 1.4, and transition to `Idle` / `CaptureVarPrompt` / error-toast-then-`Idle` accordingly. On `Cancelled` → `Idle`. On `StillOpen`/`NotHandled` mirror the preset-picker handler.
- [ ] 2.4 Wire the new state into the `handle_event` dispatch match (alongside `CapturePicking` / `CaptureVarPrompt`).

## 3. Rendering

- [ ] 3.1 Add a `NotesState::CaptureFilePicking` arm in `ft/src/tui/tabs/notes/view.rs::render` calling `render_picker_popup` with a distinct title (e.g. `" quick capture · pick target note "`).

## 4. Graph tab defensive handling

- [ ] 4.1 In `ft/src/tui/tabs/graph/modals.rs::CapturePickerModal::handle_event`, handle `CaptureResult::NeedsTarget` by queuing an error toast and returning `ModalOutcome::Closed` (unreachable in practice — graph always supplies a target — but keeps the match exhaustive).

## 5. Tests

- [ ] 5.1 In `ft/src/tui/tests/synthesis.rs`, add an append preset with no `note` field (e.g. `[capture_presets.jot]`, `action = "append"`, `template = "quick-log"`, `section = "Log"`) to the `capture_preset_vault` fixture or a new fixture.
- [ ] 5.2 Test: from the Notes tab, `Q` → select `jot` → assert a vault file picker appears (not an error toast).
- [ ] 5.3 Test: select a target note in the picker → assert the rendered template was appended to that note under the `Log` section, and the picker dismissed.
- [ ] 5.4 Test: `Esc` in the file picker returns to idle with no append and no toast.
- [ ] 5.5 Test: a no-`note` append preset whose template has `{{ vars.* }}` opens the file picker first, then the var prompt, then commits.

## 6. Build invariants

- [ ] 6.1 `cargo build --release`
- [ ] 6.2 `cargo test --workspace`
- [ ] 6.3 `cargo clippy --workspace --tests -- -D warnings`
- [ ] 6.4 `cargo fmt --check`
- [ ] 6.5 `cargo run --release -q -- commands docs --check`
