## Context

The `extract-modal-driver` change put modal dispatch behind a uniform `Modal` trait, but key dispatch inside each tab and modal is still ad-hoc match arms. The `?` overlay is hand-curated via `HelpSection` structs sitting next to those match arms; the two drift whenever one is edited without the other.

Outside the TUI, there is no programmatic way to invoke an action. The same `complete this task` operation in the TUI's Tasks tab is reachable from the CLI as `ft tasks complete <selector>`, but the two share no plumbing. Cross-cutting actions like "open this note from a hyperlink in another app" or "complete the task on this line" have no single name.

A Command/Keymap separation solves all three problems with one mechanism:

- A `Command` is a named, scoped operation. The name is stable; the dispatcher resolves it on the active context.
- A `KeyMap` is a data table from chord to command. Bindings are looked up; no `match` statements per tab.
- A `CommandRegistry` is the union of every tab's and modal's commands. It powers introspection (`ft commands list`), the `?` overlay, `docs/keybindings.md`, and the eventual user keymap config.

## Goals / Non-Goals

**Goals:**

- Every TUI action has a stable name (`<context>.<verb>`).
- Every key binding is a row in a `KeyMap`, not a match arm.
- One registry; one `?` renderer; one docs generator.
- `ft do <command>` invokes single-shot commands from outside the TUI.
- `ft commands list` introspects the registry.
- Snapshot-stable `?` overlay content. Users see the same keys and descriptions.
- The `Modal` trait from `extract-modal-driver` adopts the same pattern.

**Non-Goals:**

- No configurable keymaps in this change. The data shape supports it; the loader is a future change.
- No full headless TUI invocation: `ft do <flow-command>` for a multi-step flow is out of scope. Commands tagged `opens_modal = true` reject `ft do` with a clear error in this change.
- No new commands beyond what already exists as TUI actions. Renaming and re-grouping the existing actions only.
- No vim-style command-line / `:command` palette in the TUI itself. Possible later, not now.

## Decisions

### `Command` is a value, `CommandDef` is the metadata

```rust
// ft/src/tui/command.rs
pub struct Command {
    pub name: &'static str,        // e.g. "graph.create-note"
    pub args: CommandArgs,         // optional structured args
}

pub struct CommandDef {
    pub name: &'static str,
    pub description: &'static str,
    pub args_schema: &'static [ArgSpec],
    pub opens_modal: bool,         // gates `ft do`
    pub scope: CommandScope,       // Global, Tab("graph"), Modal("create"), …
}

pub enum CommandArgs {
    None,
    Inline(SmallVec<[(&'static str, String); 4]>),
}

pub enum CommandOutcome {
    Handled,
    NotHandled,
    OpenModal(ActiveModal),
    OpenInEditor { path: PathBuf, line: usize },
    Toast { text: String, style: ToastStyle },
    Quit,
    // … one variant per existing AppRequest variant
}
```

Commands are values (cheap to construct, cheap to compare) referencing `CommandDef`s registered at compile time. Args are sparse and inline because commands aren't allocated per keystroke.

**Alternative considered: enum of every command.** Rejected — adding a command would touch a central enum and force every dispatcher to match every variant. The string-keyed dispatch lets each tab handle only its own commands and return `NotHandled` for the rest.

### Mixed-granularity commands

Top-level commands are flow *entry points* and atomic actions:

- `graph.create-note`, `graph.append-template`, `graph.quick-capture`, `graph.rename`, `graph.move-section`, `graph.related`, `graph.search`, `graph.preset-pick`
- `tasks.complete-current`, `tasks.cycle-priority`, `tasks.set-due`, `tasks.delete-current`
- `notes.open-fuzzy`, `notes.today`, `notes.periodic`
- `app.quit`, `app.next-tab`, `app.prev-tab`, `app.switch-tab`, `app.help`
- `app.sync-git`

Inside a modal, *generic verbs* are interpreted by the active modal:

- `modal.confirm`, `modal.cancel`, `modal.next`, `modal.prev`, `modal.toggle`, `modal.delete`, `modal.up`, `modal.down`

So the section-move modal's "Space toggles selection" binding becomes `("Space" → modal.toggle)` in the modal's keymap; the modal's `dispatch_command("modal.toggle")` is the existing toggle logic. The reason this works is that, by the time `modal.toggle` is dispatched, the active modal is the section-move heading-multiselect — the verb is unambiguous in context.

**Alternative considered: every step has its own named command.** Rejected — `graph.create-note.confirm` is more specific but produces N × M commands for N flows × M steps. The modal-verb scheme keeps the registry shallow without losing locality (each modal owns its `dispatch_command`).

### Per-tab and per-modal declarations

```rust
impl Tab for GraphTab {
    fn commands(&self) -> &'static [CommandDef] { &GRAPH_COMMANDS }
    fn keymap(&self) -> KeyMap { GRAPH_KEYMAP.clone() }
    fn dispatch_command(&mut self, cmd: &Command, ctx: &mut TabCtx) -> CommandOutcome { … }
}

static GRAPH_COMMANDS: &[CommandDef] = &[
    CommandDef { name: "graph.create-note", description: "Create a new note", … },
    CommandDef { name: "graph.search", description: "Fuzzy-search nodes in the active view", … },
    …
];

static GRAPH_KEYMAP: Lazy<KeyMap> = Lazy::new(|| KeyMap::new()
    .bind("c", "graph.create-note")
    .bind("C", "graph.create-note", &[("from_selection", "true")])
    .bind("f", "graph.search")
    .bind("/", "graph.query-bar")
    …
);
```

The registry is the union of every tab's + modal's + app-global's `commands()`. Build-time composed; no runtime registration.

### Key chord representation

```rust
pub struct KeyChord {
    pub code: KeyCode,           // KeyCode::Char('c') etc.
    pub mods: KeyModifiers,
}
```

Chord strings (`"Ctrl+Shift+a"`, `"Space"`, `"Esc"`) parse via a small `chord_from_str` helper. Round-trip stable so `docs/keybindings.md` renders identically to what `?` shows. No chord *sequences* (`g s`) in this change — the existing periodic-leader chord pattern continues as a `ActiveModal::PeriodicLeader` modal that opens on `p` and dispatches commands on the second key. Multi-key sequences as first-class keymap entries are out of scope.

### Input pipeline

```text
event arrives
  ├─ active_modal active?
  │   ├─ yes → modal.keymap().lookup(chord) → Command or NotHandled
  │   │       └─ if Some(cmd): modal.dispatch_command(cmd) → ModalOutcome
  │   │       └─ if None: also dispatch raw event for cases like edit-buffer typing
  │   │
  │   └─ no  → fall through
  ├─ active tab
  │   ├─ tab.keymap().lookup(chord) → Command
  │   └─ if Some(cmd): tab.dispatch_command(cmd) → CommandOutcome
  │
  ├─ App global
  │   ├─ global_keymap().lookup(chord) → Command
  │   └─ if Some(cmd): app.dispatch_command(cmd) → CommandOutcome
```

Two layers of resolution: modal → tab → global, just like dispatch in `extract-modal-driver`. The new bit is that each layer's resolution is a `KeyMap` lookup, not a match arm.

### `?` overlay generated from keymaps

The current `HelpSection` struct (`ft/src/tui/help.rs`) accepts pre-built rows. After this change, it accepts a `&KeyMap` and renders rows from `(chord, command_name, command.description)`. Sections are derived from `CommandDef.group` (e.g., "Navigation", "Mutations", "Modals"), which is added to `CommandDef`.

The user-visible content is unchanged. The data source is unified.

### `docs/keybindings.md` generation

A new build step (run via `ft completions docs` or a separate `ft man --keybindings` flag) walks the registry, groups by scope and section, and writes a markdown file:

```markdown
# Keybindings

## App-global

| Key | Command | Description |
| --- | --- | --- |
| `Tab` | `app.next-tab` | Cycle to the next tab |
| `q` | `app.quit` | Quit the TUI |
…

## Graph tab

…

## Modals

### Section move (heading multi-select)

| Key | Command | Description |
| --- | --- | --- |
| `Space` | `modal.toggle` | Toggle the focused heading |
…
```

CI checks the committed file is in sync (`ft completions docs --check`).

### `ft do` is single-shot only in v1

```sh
ft do app.quit                    # error: app.quit needs a TUI session
ft do tasks.complete-by-id xyz123 # works — atomic, no modal
ft do graph.create-note           # error: opens_modal = true
ft do notes.rename foo bar        # works — same as `ft notes rename foo bar` but via dispatch
```

The `opens_modal: bool` flag on `CommandDef` gates `ft do`. Existing CLI subcommands (`tasks complete`, `notes rename`, `git sync`) keep their dedicated entry points; `ft do` is the orthogonal, command-registry-driven entry point. Where the two overlap, `ft do` delegates to the same library calls.

**Alternative considered: replace CLI subcommands with `ft do`.** Rejected — `ft tasks complete` is the discoverable, documented surface; `ft do tasks.complete-by-id` is the introspective fallback. Keep both.

### `ft commands list`

```sh
ft commands list --format table          # default: terminal table grouped by scope
ft commands list --format ndjson         # one JSON command per line
ft commands list --scope tab             # filter by scope
ft commands list --opens-modal false     # filter by ft-do eligibility
```

Useful for tab completion (a future shell-completion script that knows command names), debugging, and as documentation.

### Modal-state hint in the status bar

The status-bar modal indicator from `extract-modal-driver` shows the modal name. This change extends it to render two or three primary chords from the modal's keymap (chosen by a `CommandDef.is_primary` flag), so users see context-relevant hints without opening `?`. Example: section-move heading multi-select shows `Space:toggle  Enter:confirm  Esc:cancel`.

## Risks / Trade-offs

- **[Snapshot churn in `?` overlay rendering]** → Mitigation: keep the renderer's output byte-stable. Group ordering, key sorting, and description text are explicit. Existing `?` snapshots re-bless once with documented diffs (description text might be edited slightly when consolidating into `CommandDef.description`).
- **[Registry is `&'static`, dynamism is limited]** → Acceptable. Configurable keymaps load via a separate `Vec<(KeyChord, Command)>` overlay applied after the static map. The registry of commands stays static; the binding side is the only thing user-overridable later.
- **[`ft do` overlap with existing subcommands]** → Both paths exist intentionally. Documented as: "`ft tasks complete` is the discoverable command; `ft do` is the programmatic fallback." Test coverage ensures the two paths produce identical observable effects.
- **[Modal verbs vs flow-step commands]** → The decision is "modal verbs win for in-flow steps." If a future flow needs a step-specific command, add it under the modal's namespace (e.g., `create.preview-template`). Documented in `docs/commands.md`.
- **[Chord sequences (`g s`) are unfirst-class]** → Acceptable for v1. Leader-style sequences continue as transient modals (`PeriodicLeader`) which is conceptually cleaner anyway.
- **[Argument parsing for `ft do --arg key=value`]** → Simple `key=value` pairs only. No JSON, no nested args. Matches the `ArgSpec` shape (flat list of typed key→value). Documented; extensible later.

## Open Questions

- Should `ft do` emit structured output (JSON outcome) by default for scriptability? **Leaning:** yes, with `--format text` for human use and `--format json` (default when `--json-errors` is set globally) for scripts.
- Should keymap entries support `when` clauses (a predicate over context)? **Leaning:** not in this change. The scope (global/tab/modal) is enough today. Add `when` only when a real binding needs it.
- Should `docs/keybindings.md` be auto-regenerated by a pre-commit hook? **Leaning:** no — keep it manual (`ft completions docs`), with a CI check that catches drift.
