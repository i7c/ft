## Why

Creating new notes from templates is well-supported (`ft notes create --template`, TUI `C`), but there is no template-driven way to append content to an *existing* note. Users who maintain running logs (e.g. session notes, daily journals, meeting minutes) must manually open the target file, navigate to the right section, and paste or type the content. Quick capture — the ability to fire off a templated entry into a pre-configured note with a single keypress — compounds this gap: the user wants to capture a thought without breaking flow for file selection, template choice, or section navigation.

## What Changes

- **Append with template**: A new operation that renders a template and appends the result into an existing note, defaulting to end-of-file. A per-note frontmatter key (`ft-append-section`) lets the user target a specific markdown section heading instead.
- **Quick capture presets**: A new config section `[capture_presets.<name>]` where each preset bundles an action (create or append), a target specification (filename pattern for create, note path for append), and a template. From the TUI, a single keystroke invokes the preset — no template prompt, no file prompt when the note is pre-configured or derivable from selection.
- **TUI surfaces for both features**: Append-from-template is available from the graph tab (target = selected note) and the notes tab (target = picker). Quick capture is available from the graph tab (selected note for append presets, selected folder for create presets) and notes tab (picker for append presets, folder picker for create presets).
- **Post-operation editor jump**: After an append or quick-capture create, the editor opens at the line where content was inserted (not just line 1).

## Capabilities

### New Capabilities

- `append-template`: Render a template and append the result to an existing note, targeting a specific section heading via frontmatter or defaulting to end-of-file. Available via CLI subcommand and TUI keybindings.
- `quick-capture`: Config-driven one-shot presets that combine an action (create/append), a target resolution strategy, and a template. Invoked from TUI with a single keystroke; no interactive prompts for template or (when derivable) target.

### Modified Capabilities

<!-- None: these are entirely new capabilities with no existing spec-level behavior changes. -->

## Impact

- **ft-core**: New `notes::append` module (template-driven append to existing files), new `capture` module (preset model, resolution logic). `Config` gains a `capture_presets` field. `notes::template::TemplateContext` likely reused as-is.
- **ft binary**: New `ft notes append` subcommand. New quick-capture TUI flow wired into graph tab and notes tab keybindings.
- **TUI**: New keybindings on graph tab and notes tab for append-with-template and quick-capture invocation. New quick-capture preset picker widget.
- **No breaking changes**. All existing subcommands, keybindings, and config formats are untouched.
