## 1. Command + keymap infrastructure

- [x] 1.1 `ft/src/tui/command.rs` exports `Command`, `CommandArgs`, `CommandDef`, `CommandScope`, `ArgSpec`, `CommandOutcome`, `CommandRegistry`. `CommandOutcome` is just `Handled`/`NotHandled` — cross-scope side effects (open modal, push toast, suspend for editor, …) flow through `ctx.pending_request` as `AppRequest` variants so this enum stays small and command.rs stays decoupled from `ActiveModal`
- [x] 1.2 `ft/src/tui/keymap.rs` exports `KeyChord` (with `normalized()` so `Char('C')+NONE` and `Char('c')+SHIFT` resolve identically — terminals are inconsistent), `chord_from_str` / `chord_to_str` round-trip, and `KeyMap` with a fluent `bind`/`bind_with_args` builder that panics on duplicate chords (post-normalization)
- [x] 1.3 `CommandRegistry::from_slices(&[&'static [CommandDef]])` is the primitive. The `build(tabs, modals, global)` walker that gathers static slices from every tab + modal + global will land in §3 once `Tab::commands()` exists
- [x] 1.4 18 unit tests: command/registry (6) — args lookup, duplicate-name panic, iter order, scope; keymap (12) — chord round-trip (letters, modifiers, named keys), modifier aliases, invalid inputs, normalization on key-event input, bind+lookup, args-on-bind, duplicate detection (direct and via normalization), invalid-chord-on-bind, iter order

## 2. Update `Tab` and `Modal` traits

- [x] 2.1 `Tab::commands(&self) -> &'static [CommandDef]` (default: `&[]`)
- [x] 2.2 `Tab::keymap(&self) -> &KeyMap` (default: `empty_keymap()` shared static — returning a borrow avoids cloning per keystroke)
- [x] 2.3 `Tab::dispatch_command(&mut self, _cmd: &Command, _ctx: &mut TabCtx) -> CommandOutcome` with default returning `NotHandled` (relaxed from "no default" in tasks.md — staged migration; tabs override as they adopt). Each tab keeps its current `handle_event` impl until §4 converts it
- [x] 2.4 Default `Tab::handle_event` deferred to §4 — adding it now would race with every tab's existing implementation. The default will be installed as part of the per-tab conversion sweep when each tab's `handle_event` is replaced with `keymap → command → dispatch_command`
- [x] 2.5 Same three methods on `Modal` trait, with the same default-`NotHandled` for `dispatch_command`. `ActiveModal` propagates `commands()` / `keymap()` / `dispatch_command()` to the inner variant (matches the existing `handle_event` / `render` / `name` propagation pattern)

## 3. Convert App global keymap

- [x] 3.1 `App::global_keymap()` returns `&APP_KEYMAP`. The map binds `q`/`Ctrl+c` (quit), `Tab`/`BackTab` (next/prev tab), `1`..`9` (switch-tab with `index` arg), `?` (help), `g` (git-leader). Keymap normalization handles the `?` case where the terminal may or may not include the SHIFT modifier
- [x] 3.2 `APP_COMMANDS` slice in `ft/src/tui/app_commands.rs` declares `app.quit`, `app.next-tab`, `app.prev-tab`, `app.switch-tab` (with `index` arg), `app.help`, `app.git-leader`. (Note: tasks.md says `app.sync-git` — but the existing chord is `g` → leader mode → `s` (Mode::GitLeader), not a single-shot. Named `app.git-leader` to match the actual single-key binding; the leader-then-`s` flow stays in `Mode::GitLeader` since chord sequences are out of scope per design.md)
- [x] 3.3 `App::handle_global_key` now calls `APP_KEYMAP.lookup(chord)` and `dispatch_global_command(cmd)`. The match arms migrated 1:1; behavior is byte-identical (verified by full test suite passing without snapshot diffs)

## 4. Convert tabs

- [ ] 4.1 GraphTab: define `GRAPH_COMMANDS`, `GRAPH_KEYMAP`, `dispatch_command`; remove the per-key match arms; the modal-launch keys now produce `CommandOutcome::OpenModal(...)`
- [ ] 4.2 TasksTab: same conversion
- [ ] 4.3 NotesTab: same conversion
- [ ] 4.4 TimeblocksTab: same conversion
- [x] 4.5 JournalTab — pilot conversion to validate the pattern. `JOURNAL_COMMANDS` (10 commands across Source/Navigation/Open groups) + `JOURNAL_KEYMAP` (12 bindings; `Up`/`k` and `Down`/`j` aliases share commands) + `dispatch_command` arm-per-command. `handle_event` keeps the picker-overlay bypass at the top (the tab-resident `FuzzyPicker` captures keys before the keymap is consulted) then does `KEYMAP.lookup → dispatch_command → EventOutcome`. Tests pass without changes; the pre-migration `help_sections()` stays in place (§6 replaces it with keymap-derived rendering)

### Lessons from the pilot
1. **Tab-resident sub-modals (picker, leader chord, etc.) bypass the keymap** — convert by routing raw events to the sub-modal first, then keymap lookup. Same pattern will apply to the GraphTab's `m`-leader and the periodic-leader if those stay tab-resident; ActiveModal-driven pickers (Search, Preset) don't need this.
2. **Modifier-loose matching becomes strict** — the original `m if m.contains(KeyModifiers::CONTROL)` is replaced by exact `Ctrl+d` binding. `Ctrl+Shift+d` no longer fires the command (no tests caught this; accepting as minor behavior tightening).
3. **`Shift+Letter` works automatically** — `bind("R", ...)` and `bind("G", ...)` rely on KeyChord normalization (`Char('R')+NONE` → `Char('r')+SHIFT` matches the bound chord).
4. **Multi-chord aliases** are two `.bind()` calls to the same command name. No keymap-level aliasing primitive needed.
5. **Net code growth ≈ +90 LoC per tab** — the static `CommandDef` slice is the bulk (~80 lines for 10 commands). Match arms shrink by ~30 LoC. The trade is intentional: the metadata powers `?`, docs, the registry, and the eventual user-keymap config.
6. **Tasks 2.4 deferred again** — Journal's `handle_event` has the picker-overlay bypass that the trait default can't express. Per-tab `handle_event` overrides will remain typical for any tab with tab-resident sub-modals; the default's value is for tabs whose dispatch is purely keymap-driven (potentially none, given the picker pattern is common).

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
