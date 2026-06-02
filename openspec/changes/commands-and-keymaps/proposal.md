## Why

Today's TUI keymap is encoded as scattered `match KeyCode::Char('c')` arms across 4k+ lines. There is no single source of truth for "what key does what here" — the `?` overlay is hand-curated, `docs/keybindings.md` doesn't exist, and there is no way to invoke a TUI action from outside the TUI (scripting, agent control, accessibility, future user keymap config). Adding a new chord means editing both the match arm and the help text, and they drift.

This change introduces a Command/Keymap layer. Every action becomes a named `Command` (`<context>.<verb>` string). Each context (App-global, per-tab, per-modal) declares a `KeyMap` mapping key chords to commands. The TUI input pipeline becomes: key → keymap lookup → command dispatch on active context. The `?` overlay and `docs/keybindings.md` are both generated from the same registry. `ft do <command>` invokes single-shot commands without entering the TUI. `ft commands list` lists every command in the registry.

This change sequences immediately after `extract-modal-driver`, which provided the modal dispatch interface this change names and elevates.

## What Changes

- New module `ft/src/tui/command.rs`: `Command` (a value-typed reference: namespace string + optional args), `CommandDef` (name, description, parameter schema), `CommandRegistry` (build-time-composed map from name to dispatch handler).
- New module `ft/src/tui/keymap.rs`: `KeyChord` (key code + modifiers), `KeyMap` (`Vec<(KeyChord, Command)>` with collision detection), `KeyMapScope` (`Global`, `Tab(&'static str)`, `Modal(&'static str)`).
- `Tab` trait gains `commands() -> &'static [CommandDef]` and `keymap() -> KeyMap`, plus `dispatch_command(&mut self, cmd: &Command, ctx: &mut TabCtx) -> CommandOutcome`.
- `Modal` trait (from `extract-modal-driver`) gains the same three methods. The modal's `handle_event` becomes a thin function that resolves key → command → dispatches via `dispatch_command`.
- App-global keymap lives in `app.rs::App::global_keymap()`. It owns: tab cycling, quit, help, and any other always-on bindings.
- Every existing binding in every tab + modal is converted to the new model. Mixed granularity: top-level commands launch flows (`graph.create-note`); inside a modal, generic verbs (`modal.confirm`, `modal.cancel`, `modal.next`, `modal.toggle`) are interpreted by the active modal's `dispatch_command`.
- The `?` overlay reads from the active context's keymap + the modal's (if any) + the global's. No more hand-maintained `HelpSection` structs (the data is in the keymap; `HelpSection` becomes a renderer over `KeyMap`).
- New CLI subcommand `ft commands list` introspects the registry and prints names + descriptions (table / json / ndjson formats).
- New CLI subcommand `ft do <command> [--arg key=value …]` dispatches a command. The first implementation only accepts non-modal-opening commands (e.g., `tasks.complete-by-id`, `notes.rename`, `graph.refresh`); commands tagged `opens_modal = true` exit with a clear "this command launches an interactive flow; use `ft tui` for now" message. The seam for full headless flow execution is reserved.
- Auto-generated `docs/keybindings.md` written by `ft man --out` (extended) or a new `ft completions docs` subcommand. The CI invariant adds a check that the generated file matches the committed version.
- Status-bar modal indicator (from `extract-modal-driver`) extends to show the active modal's primary keymap hints (e.g., `Enter:confirm  Esc:cancel`) so users see context-relevant chords without opening `?`.

## Capabilities

### New Capabilities

- `tui-commands`: Every TUI action is a `Command` identified by a `<context>.<verb>` name. Commands have descriptions and optional parameter schemas. A central `CommandRegistry` enumerates every command in the binary.
- `tui-keymaps`: Key bindings live in scoped `KeyMap`s (global, per-tab, per-modal). The TUI input pipeline resolves chords to commands and dispatches them on the active context. The `?` overlay and `docs/keybindings.md` are generated from these keymaps.
- `cli-do`: A `ft do <command>` subcommand dispatches non-modal commands without entering the TUI. A `ft commands list` subcommand introspects the registry.

### Modified Capabilities

- `help-per-tab`: The `?` overlay is now generated from `KeyMap` data instead of hand-curated `HelpSection` structs. The behaviour (per-tab plus global) is unchanged from the user's perspective; the implementation is data-driven.
- `tui-modal-driver`: The `Modal` trait gains `commands()`, `keymap()`, `dispatch_command()` methods. Modal `handle_event` becomes a default impl that resolves key → command → dispatches.

## Impact

- **New**: `ft/src/tui/command.rs` (≈ 300 lines), `ft/src/tui/keymap.rs` (≈ 200 lines), `ft/src/cmd/commands.rs` (`ft commands list`), `ft/src/cmd/do.rs` (`ft do`).
- **Modified**: `ft/src/main.rs` (two new subcommand variants), `ft/src/tui/tab.rs` (Tab trait gains three methods; `HelpSection` becomes a renderer), `ft/src/tui/modal.rs` (Modal trait gains three methods; default `handle_event`), every tab + modal source file (key arms become `dispatch_command` arms; old `HelpSection` constructors deleted).
- **Generated**: `docs/keybindings.md` (committed; CI checks freshness).
- **Tests**: Snapshot baseline preserved for the `?` overlay content (same key/description lines, just sourced differently); new tests cover registry completeness, keymap collision detection, `ft commands list` output, `ft do` dispatch of a non-modal command, the "opens_modal" guard, and the docs-freshness check.
- **Docs**: New `docs/commands.md` documenting the command/keymap model; `docs/keybindings.md` generated.
- All four build invariants stay green.
