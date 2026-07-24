## Context

Quick-capture append presets resolve their target note from one of two
sources: a hardcoded `note` field, or the invoking tab's context. The
Graph tab supplies the currently-selected note as a `target_note_override`
into `capture::try_execute_preset`; the Notes tab has no single "selected
note" and is spec'd to open a vault file picker instead.

Today the Notes tab passes `None` as the override, and
`resolve_append_target` treats `(None, None)` as a hard error:

```rust
(None, None) => Err("no target note for append preset".to_string()),
```

That error propagates as a toast. There is no code path that opens a
picker — `CaptureResult` only has `Executed` and `NeedsVars`, both of
which require a resolved target up front. The append-with-template flow
(`notes_actions/append.rs`) already implements the exact two-phase
pattern we need (`AppendState::FilePicking` over `VaultFilePickerSource`),
so this is a matter of mirroring it in the capture flow.

The existing spec (`openspec/specs/quick-capture/spec.md`, "Notes tab
opens file picker") already requires this behavior; the implementation
never matched it.

## Goals / Non-Goals

**Goals:**
- An append capture preset with no `note` field, invoked from the Notes
  tab, opens a vault file picker and appends to the chosen note.
- The existing var-prompt flow still works when the template has
  `{{ vars.* }}` references — the picker runs first, then vars.
- The Graph tab is untouched (it always supplies a target).
- `section` override and `ft.append.section` frontmatter resolution are
  unchanged.

**Non-Goals:**
- No changes to create presets (they already have their own path/folder
  resolution, including a not-yet-wired filename prompt — out of scope).
- No changes to the CLI (`ft notes append` already takes an explicit
  path argument).
- No new config fields. The `note` field's absence remains the signal.
- No changes to `ft-core`; this is purely a TUI flow fix.

## Decisions

### Decision: New `CaptureResult::NeedsTarget` variant

`try_execute_preset` currently returns `Err(String)` when an append
preset has no resolvable target. We add a third outcome:

```rust
pub enum CaptureResult {
    Executed,
    NeedsVars(CaptureVarPromptState),
    NeedsTarget(CaptureTargetPromptState),  // new
}
```

`NeedsTarget` carries everything needed to commit once a target is
chosen: the template source, the `section_override`, and `vars_needed`.
It does **not** carry a `target_path` (that's the whole point).

**Why a new variant over reusing `Err`:** `Err` is already the
"surface a toast and bail" channel. A picker is a legitimate
non-error continuation; conflating it with errors would force callers
to string-match to distinguish "no target, open picker" from "template
not found, show toast." A typed variant keeps the dispatch clean.

**Alternative considered:** have `try_execute_preset` take a closure
or callback that opens the picker. Rejected — the capture flow is
already split across `capture.rs` (logic) and the tab (state ownership +
rendering), and a callback would invert that boundary and make the
picker hard to render/test. The `NeedsTarget` return keeps the tab in
charge of its own state machine, matching how `NeedsVars` already works.

### Decision: Partial-commit payload type

`CaptureCommit` currently bundles `target_path` with everything else.
For the picker phase the target is unknown, so we introduce a
`CaptureTargetPromptState` holding the subset without `target_path`:

```rust
pub struct CaptureTargetPromptState {
    pub template_source: String,
    pub section_override: Option<String>,
    pub vars_needed: Vec<String>,
    pub picker: FuzzyPicker<VaultFilePickerSource>,
}
```

On selection, the handler resolves the absolute path, builds a full
`CaptureCommit`, and either commits (no vars) or transitions to the
existing `CaptureVarPrompt` state (vars present). This mirrors
`AppendState::FilePicking` → `VarPrompt` in `append.rs`.

**Why not reuse `CaptureCommit` with `target_path: Option<PathBuf>`:**
`CaptureCommit` is also the post-resolution payload passed to
`commit_capture`, where `target_path` is non-optional by contract.
Making it optional there would push an unwrap into the commit path.
A separate prompt-state type keeps each phase's invariants explicit.

### Decision: New `NotesState::CaptureFilePicking` variant

The Notes tab already has `CapturePicking` (preset picker) and
`CaptureVarPrompt`. We add:

```rust
CaptureFilePicking(CaptureTargetPromptState),
```

with a dispatch arm, a `handle_capture_file_picker_key` handler, and a
render branch reusing the existing `render_picker_popup` (same popup
the preset picker uses, different title). `Esc` cancels back to idle;
`Enter` resolves and continues.

The Graph tab needs no new state — `CapturePickerModal` always receives
a `target_note_override`, so `try_execute_preset` never returns
`NeedsTarget` there. We still handle `NeedsTarget` defensively in the
Graph modal (treat as an error toast) so a future caller can't panic,
but it's unreachable in practice.

### Decision: Picker source is `VaultFilePickerSource`

Same source the append flow and the Notes `o` open-picker use —
recents-weighted, vault-relative, with path + content matching. Reusing
it means no new picker infrastructure and consistent UX across all
"pick a note" surfaces.

## Risks / Trade-offs

- **[Graph modal receives `NeedsTarget` and must not panic]** → Handle
  the variant in `CapturePickerModal::handle_event` by queuing an error
  toast and closing. Unreachable today, but keeps the match exhaustive.
- **[Existing tests assert `Err` on no-target]** → The current
  `resolve_append_target` `(None, None)` arm returns `Err`; any test
  relying on that string changes. Grep shows no such test (the synthesis
  tests use presets *with* `note`), so this is low-risk. The new
  behavior is covered by a fresh test.
- **[Two-phase flow doubles the states a user can be in]** → Mitigated
  by reusing the exact render/handle shape of the preset picker and the
  append file picker, so the `?` overlay and keymap stay consistent. No
  new keymap overlay needed — the picker inherits the capture-picker
  keymap's `Enter`/`Esc` semantics.
- **[Picker shows non-`.md` or non-note files]** → `VaultFilePickerSource`
  already filters to vault notes; no new filtering needed.
