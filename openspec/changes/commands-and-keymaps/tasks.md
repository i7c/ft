## 1. Command + keymap infrastructure

- [ ] 1.1 Create `ft/src/tui/command.rs` with `Command`, `CommandDef`, `CommandArgs`, `ArgSpec`, `CommandScope`, `CommandOutcome`, `CommandRegistry`
- [ ] 1.2 Create `ft/src/tui/keymap.rs` with `KeyChord` (`code: KeyCode`, `mods: KeyModifiers`), `chord_from_str` parser + round-trip helper, `KeyMap` (Vec of (chord, command) with `bind(chord, command_name, args?)` builder, panics on duplicate chords)
- [ ] 1.3 Add registry composition: `CommandRegistry::build(tabs: &[Box<dyn Tab>])` walks every tab + modal + global commands, asserts unique names, returns a queryable registry
- [ ] 1.4 Unit tests: chord parse/format round-trip; KeyMap duplicate detection; registry build with synthetic tabs

## 2. Update `Tab` and `Modal` traits

- [ ] 2.1 Add `Tab::commands(&self) -> &'static [CommandDef]` (default: `&[]`)
- [ ] 2.2 Add `Tab::keymap(&self) -> KeyMap` (default: empty)
- [ ] 2.3 Add `Tab::dispatch_command(&mut self, cmd: &Command, ctx: &mut TabCtx) -> CommandOutcome` (no default — every tab implements)
- [ ] 2.4 Provide a default `Tab::handle_event` that resolves chord → command → dispatches (tabs can still override for raw events like edit-buffer text input)
- [ ] 2.5 Same three methods on `Modal` trait; default `Modal::handle_event` resolves chord → command → dispatches

## 3. Convert App global keymap

- [ ] 3.1 Add `App::global_keymap()` returning the global KeyMap (Tab, Shift+Tab, 1-5, q, Ctrl+C, ?, g s, etc.)
- [ ] 3.2 Define `app.*` commands in a new `app_commands` constant: `app.quit`, `app.next-tab`, `app.prev-tab`, `app.switch-tab` (with `index` arg), `app.help`, `app.sync-git`
- [ ] 3.3 Move the global match arms in `App::handle_event` to call `global_keymap().lookup(chord)` + `dispatch_command`

## 4. Convert tabs

- [ ] 4.1 GraphTab: define `GRAPH_COMMANDS`, `GRAPH_KEYMAP`, `dispatch_command`; remove the per-key match arms; the modal-launch keys now produce `CommandOutcome::OpenModal(...)`
- [ ] 4.2 TasksTab: same conversion
- [ ] 4.3 NotesTab: same conversion
- [ ] 4.4 TimeblocksTab: same conversion
- [ ] 4.5 JournalTab: same conversion

## 5. Convert modals

- [ ] 5.1 Each `ActiveModal` variant gets a static `<MODAL>_COMMANDS` + `<MODAL>_KEYMAP`; `dispatch_command` matches on command name
- [ ] 5.2 Modal commands include the generic verb set (`modal.confirm`, `modal.cancel`, `modal.next`, `modal.prev`, `modal.toggle`, `modal.delete`, `modal.up`, `modal.down`) where applicable
- [ ] 5.3 Picker variants (`PresetPicker`, `CapturePicker`, `Search`) share a common `Picker` keymap module to avoid copy-paste

## 6. `?` overlay regen

- [ ] 6.1 Refactor `HelpSection` to consume `(&KeyMap, &CommandRegistry)` and render `(chord, command_name, description)` rows grouped by `CommandDef.group`
- [ ] 6.2 Remove every hand-curated `HelpSection` builder; the renderer is the only path
- [ ] 6.3 Snapshot test: `?` overlay on every tab matches its previous render byte-for-byte (re-bless once for description text consolidations, document each diff in PR)
- [ ] 6.4 Snapshot test: `?` overlay while each `ActiveModal` variant is active shows the modal's keymap, not the tab's

## 7. `docs/keybindings.md` generation

- [ ] 7.1 Implement `ft completions docs` (or `ft man --keybindings`) that walks the registry and writes a markdown file
- [ ] 7.2 Commit the generated file under `docs/keybindings.md`
- [ ] 7.3 Add CI invariant: `ft completions docs --check` exits zero iff the committed file matches the generator output
- [ ] 7.4 Test: integration test invoking the generator against a fixture App and asserting structure

## 8. `ft commands list`

- [ ] 8.1 New module `ft/src/cmd/commands.rs` with `CommandsArgs`, `run` dispatching to `list`
- [ ] 8.2 Register `Commands` subcommand variant in `main.rs`
- [ ] 8.3 Implement `--format table|json|ndjson`, `--scope`, `--opens-modal` filters
- [ ] 8.4 Snapshot tests: each format on a fixture registry

## 9. `ft do <command>`

- [ ] 9.1 New module `ft/src/cmd/do.rs` with `DoArgs { command: String, args: Vec<String>, format: OutputFormat }`
- [ ] 9.2 Parse `--arg key=value` repeated; validate against `CommandDef.args_schema`
- [ ] 9.3 Look up the command; if `opens_modal`, reject with exit 2 and message
- [ ] 9.4 Dispatch via a shared handler that the TUI also calls (extracted from each tab's `dispatch_command` for non-modal-opening commands — see 9.5)
- [ ] 9.5 For each non-modal command, factor the handler into a free function in the appropriate module (e.g., `ft_core::task::ops::complete_task` is already library-side; `ft do` just wires args to it). The TUI's `dispatch_command` arm calls the same function
- [ ] 9.6 Implement `--format text|json` output; honour top-level `--json-errors`
- [ ] 9.7 Tests: dispatch of `tasks.complete-by-id`, rejection of `graph.create-note`, unknown-command path, missing-arg path, JSON output shape

## 10. Status-bar modal hint

- [ ] 10.1 Add `CommandDef.is_primary: bool` (defaults false)
- [ ] 10.2 In the status-bar modal indicator (introduced by `extract-modal-driver`), render up to three primary chords with their descriptions, sourced from the active modal's keymap
- [ ] 10.3 Snapshot tests for each modal's hint cell

## 11. Docs

- [ ] 11.1 New `docs/commands.md` documenting the model, command naming convention, `ft do` / `ft commands list` usage
- [ ] 11.2 Update `docs/architecture.md` with the Command/Keymap section
- [ ] 11.3 Update `README.md` quick-start to mention `ft commands list` for discoverability

## 12. Build validation

- [ ] 12.1 `cargo build --release` — clean
- [ ] 12.2 `cargo test --workspace` — all tests pass; snapshot diffs only where explicitly re-blessed
- [ ] 12.3 `cargo clippy --workspace --tests -- -D warnings` — clean
- [ ] 12.4 `cargo fmt --check` — clean
- [ ] 12.5 `ft completions docs --check` — clean in CI
