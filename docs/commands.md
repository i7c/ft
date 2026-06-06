# Commands and keymaps

The TUI's input pipeline is built on a Command/Keymap separation. Every
action the TUI can take is a named `Command`; every key binding is a row
in a `KeyMap` that maps a chord to a command. The `?` overlay,
[docs/keybindings.md](keybindings.md), `ft commands list`, and `ft do`
all read from the same registry — there is one source of truth for what
exists, what each thing does, and how to trigger it.

## Concepts

### `Command` and `CommandDef`

- **`Command`** (`ft/src/tui/command.rs`) — a value: a stable
  `<context>.<verb>` name plus optional inline string args. Cheap to
  build, cheap to compare; held inside `KeyMap` entries.
- **`CommandDef`** — the metadata for one command: `name`,
  `description`, `scope`, `group`, `args_schema`, `opens_modal`,
  `is_primary`. Declared in `static` slices next to the tab or modal
  that owns the command.

Names match `[a-z][a-z0-9-]*\.[a-z][a-z0-9-]*` and are stable across
releases. Adding a new chord means adding a row to a keymap; adding a
new action means adding a `CommandDef` to one of the static slices.

### `CommandScope`

`Global` (App-wide), `Tab(name)`, or `Modal(name)`. Scope drives:

- `?` overlay grouping (global section first, then the active context).
- `docs/keybindings.md` section ordering.
- `ft commands list --scope <s>` filter resolution.

### `KeyMap` and `KeyChord`

`KeyChord` is a normalized `(KeyCode, KeyModifiers)`. Normalization
collapses terminal inconsistencies — `Char('C')+NONE` and
`Char('c')+SHIFT` resolve to the same chord; `?` with or without SHIFT
likewise. Built via `chord_from_str("Ctrl+Shift+a")` /
`chord_to_str(chord)` so chord strings round-trip stably.

`KeyMap` is a small `Vec<(KeyChord, Command)>` built by a fluent
`.bind("c", "graph.create-note")` builder. Duplicate chords inside one
map panic at construction time so collisions surface immediately
during `cargo build`. Cross-scope duplicates (a global binding sharing
a chord with a tab binding) are allowed — precedence resolves which
fires.

### Input pipeline

Key events resolve modal → tab → global. Each layer is a keymap lookup
followed by a `dispatch_command(cmd, ctx)`. The first layer to return
`Handled` consumes the event.

```text
key event
  ├─ active_modal? → modal.keymap().lookup → modal.dispatch_command
  ├─ active_tab    → tab.keymap().lookup   → tab.dispatch_command
  └─ App-global    → APP_KEYMAP.lookup     → dispatch_global_command
```

Cross-scope side effects (open a modal, push a toast, suspend for the
editor, …) flow through `ctx.pending_request` as `AppRequest` variants
rather than as bigger `CommandOutcome` variants. `CommandOutcome` stays
small: `Handled` or `NotHandled`.

## Tools

### `ft commands list`

Introspects the registry.

```sh
ft commands list                         # default: terminal table grouped by scope
ft commands list --format ndjson         # one JSON command per line
ft commands list --format json           # full registry as a JSON array
ft commands list --scope tab             # filter by scope (global / tab / modal / tab/<name> / modal/<name>)
ft commands list --opens-modal false     # filter by ft-do eligibility
```

### `ft commands docs`

Walks the registry and emits the markdown reference at
[docs/keybindings.md](keybindings.md). Output is deterministic — the
same registry produces byte-identical bytes.

```sh
ft commands docs > docs/keybindings.md   # regenerate the committed file
ft commands docs --check                 # exit 0 iff the committed file matches
```

CI runs `--check` to catch drift between the registry and the
committed file.

### `ft commands check-keymap`

Validates `[keymap]` entries in `config.toml` against every known scope's
default keymap. Reports unknown commands, invalid chord strings, and chords
targeted for unbinding that don't exist in the base map.

```sh
ft commands check-keymap                 # exit 0 if clean; exit 2 on any error
ft commands check-keymap --format json   # machine-readable error list
```

Useful as a lint step after editing `config.toml`. The same validation runs
at TUI startup under `strict = true`; `check-keymap` lets you catch errors
before launching the TUI. See [docs/config.md](config.md#keymap) for the
full `[keymap]` schema.

### `ft commands list --effective`

Like `ft commands list` but composes per-scope effective keymaps — the
default bindings merged with any user `[keymap]` overlays from `config.toml`
— and emits chord-to-command rows:

```sh
ft commands list --effective             # all scopes, terminal table
ft commands list --effective --scope global  # filter to one scope
ft commands list --effective --format ndjson # machine-readable
```

### `ft do <command>`

Dispatches a registered command headlessly.

```sh
ft do tasks.complete-by-id --arg id=xyz123
ft do tasks.complete-by-id --arg id=xyz123 --format json
ft do graph.create-note                  # rejected: opens a modal
ft do unknown.command                    # rejected: not in the registry
```

Exit codes:

- `0` — success.
- `2` — usage error (unknown command, modal-opening command, missing
  required arg, unparseable arg).
- `3` — the command is registered but has no headless handler yet.

The headless path is intentionally narrow today: most registered
commands mutate TUI state (cursor position, view selection,
multi-selection toggles) and don't have a meaningful headless analog.
Atomic ops with explicit selectors — like the spec scenario
`tasks.complete-by-id --arg id=…` — are factored into shared handlers
that `ft do` calls directly. Add a new handler when the underlying
`ft-core` operation is atomic enough to invoke without the TUI's
ambient state.

## Mixed granularity

Top-level commands are flow entry points (`graph.create-note`,
`tasks.complete`) and atomic actions (`tasks.complete-by-id`,
`graph.refresh`).

Inside a modal, verbs are reified per-modal — each modal owns its
`<modal>.confirm`, `<modal>.cancel`, etc., so the registry stays
collision-free. Helpers (`confirm_def`, `cancel_def`, `nav_def` in
`ft/src/tui/modal_commands.rs`) keep the boilerplate down.

## `?` overlay

The overlay is generated from the active context's keymap and the
central registry via `help::sections_from_keymap`. Aliases (multiple
chords bound to one command) collapse onto one row joined by `" / "`.
Contiguous mod+digit runs (e.g. `Alt+1..Alt+9`) collapse to range form
so the key column stays narrow.

## Status-bar modal hint

When a modal is active, the status bar's center cell renders up to
three `chord:label` pairs picked from the modal's keymap by
`CommandDef.is_primary = true`. The right cell still shows
`modal: <name>` (the indicator from `extract-modal-driver`). Toasts
take priority over the hint cell so transient feedback isn't crowded
out.

Authors control which chords surface by setting `is_primary: true` on
the relevant `CommandDef` (the `confirm_def` and `cancel_def` helpers
do this by default) and by ordering primary commands first in the
modal's keymap chain.

## Adding commands and keymaps

Existing commands are user-rebindable at runtime — no recompilation needed.
Add a `[keymap.<scope>]` table to your `config.toml` to assign new chords,
and `[[keymap.unbind]]` entries to remove defaults. See
[docs/config.md](config.md#keymap) for the full schema, examples, and the
`ft commands check-keymap` lint tool.

### A new command on an existing tab

1. Append a `CommandDef` to the tab's `<TAB>_COMMANDS` slice in
   `ft/src/tui/tabs/<tab>/`. Pick a stable
   `<tab-name>.<verb>` name.
2. Add a chord binding to the tab's `<TAB>_KEYMAP`
   (`.bind("c", "<tab>.<verb>")`).
3. Add an arm to the tab's `dispatch_command` matching the new name.
4. Re-run `ft commands docs > docs/keybindings.md` and commit the
   updated file.

### A new modal

See [docs/architecture.md](architecture.md) §"Adding a new modal" for
the modal-trait scaffolding. After that:

1. Declare `<MODAL>_COMMANDS` and `<MODAL>_KEYMAP` in
   `ft/src/tui/modal_commands.rs`.
2. Override `Modal::commands` and `Modal::keymap` on the new variant.
3. Mark `is_primary: true` on the chords that should appear in the
   status-bar hint (`confirm_def`/`cancel_def` do this automatically).
4. Re-run `ft commands docs > docs/keybindings.md`.

### A new headless `ft do` handler

1. Add the `CommandDef` to the appropriate scope's slice with an
   accurate `args_schema`.
2. Add a match arm in `ft/src/cmd/do.rs::run` that maps the command
   name to a function that calls the underlying `ft-core` operation.
3. Add a unit test under `cmd::do_cmd::tests` exercising the
   end-to-end path against a temp vault.
4. Re-run `ft commands docs > docs/keybindings.md`.

## See also

- [docs/keybindings.md](keybindings.md) — generated reference of every
  registered command, grouped by scope.
- [docs/architecture.md](architecture.md) §"Modal driver (TUI)" — how
  modals fit into the App, including the trait scaffolding referenced
  above.
