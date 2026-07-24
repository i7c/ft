## Why

Quick-capture append presets without a hardcoded `note` field are
documented (and spec'd) to open a vault file picker when invoked from the
Notes tab. Instead they fail with an error toast ("no target note for
append preset") because the capture flow treats a missing target as a
hard error rather than a signal to prompt. The Graph tab works only
because it always supplies the selected note as an override; the Notes
tab passes `None` and hits the dead branch.

## What Changes

- The capture flow gains a `NeedsTarget` outcome so an append preset
  with no `note` and no caller-supplied target returns control to the
  caller instead of erroring.
- The Notes tab gains a `CaptureFilePicking` state that opens a
  `VaultFilePickerSource` picker; on selection it resolves the absolute
  path and either commits immediately (no template vars) or transitions
  into the existing `CaptureVarPrompt` state.
- The Graph tab is unchanged — it always supplies a target, so it never
  produces `NeedsTarget`.
- The `section` override and `ft.append.section` frontmatter resolution
  are unchanged; they ride along on the commit.

## Capabilities

### New Capabilities

_(none)_

### Modified Capabilities

- `quick-capture`: append presets without a `note` field, invoked from
  the Notes tab, SHALL open a vault file picker to choose the target
  note instead of erroring. (This is already a requirement in the
  existing spec — "Notes tab opens file picker" — but was never
  implemented; this change makes the spec and behavior agree.)

## Impact

- `ft/src/tui/notes_actions/capture.rs`: new `CaptureResult::NeedsTarget`
  variant + a partial-commit payload type; `try_execute_preset` returns
  it instead of `Err` for the no-target append case.
- `ft/src/tui/tabs/notes/mod.rs`: new `NotesState::CaptureFilePicking`
  variant, dispatch + key handler + render hookup.
- `ft/src/tui/tabs/notes/view.rs`: render branch for the new state
  (reuses the existing `render_picker_popup`).
- `ft/src/tui/tests/synthesis.rs`: new test asserting the picker opens
  and the append lands in the chosen note.
- No changes to `ft-core`, the CLI, config schema, or the Graph tab.
