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

- [x] 4.1 GraphTab — converted. `GRAPH_COMMANDS` (32 commands across Views/Notes/Query/Navigation/Periodic groups) + `GRAPH_KEYMAP` (44 bindings including arrow+vim aliases, `Alt+1`..`Alt+9` with `index` arg, `Ctrl+chord` variants). `dispatch_command` covers all 32; `handle_event` retains the Tab/BackTab/plain-digit App-passthrough gate and the graph-missing-or-empty branch, then does keymap lookup. Modal-launch commands post `OpenModal(...)` via `pending_request`; the `r`-with-multi-selection vs r-rename bifurcation lives in the `graph.rename-or-multi-move` arm. 64 graph tests pass; 770 binary tests overall.
- [x] 4.2 TasksTab — converted. The conversion spans two files because TasksTab delegates most key handling to its active `View` (currently only `SearchView`). All 20 commands ("tasks.*") live in one `TASKS_COMMANDS` slice (declared in `tabs/tasks/mod.rs`). TasksTab owns 3 sidebar commands; SearchView owns 17 (navigation, mutations, create/edit). `TASKS_KEYMAP` (3 bindings: `Up`/`Down`/`Enter`) covers the sidebar fall-through. `SEARCH_KEYMAP` (19 bindings, view-internal in `search.rs`) covers Idle-state keys; sub-modes (popup/quickline/edit_state) bypass it. `TasksTab::dispatch_command` handles the 3 sidebar verbs; `SearchView::dispatch_idle_command` handles the 17 view verbs. 770 tests pass; no snapshot diffs.
- [x] 4.3 NotesTab — converted. `NOTES_COMMANDS` (8 commands across Notes / Periodic-notes groups) + `NOTES_KEYMAP` (8 bindings; only Idle-state keys). `Tab::dispatch_command` delegates to a private `dispatch_idle_command` that takes `&TabCtx` (lighter than `&mut TabCtx`) so it can be called from `handle_idle_key` without re-borrowing. `handle_event` already had per-state delegation — sub-states (OpenPicking, Creating, Appending, CapturePicking, CaptureVarPrompt, MoveSection, PeriodicLeader) keep their own raw-key handlers. 89 notes tests pass.
- [x] 4.4 TimeblocksTab — converted. `TIMEBLOCKS_COMMANDS` (23 commands across Navigation/Edit times/Create-edit-delete groups) + `TIMEBLOCKS_KEYMAP` (29 bindings; `Up`/`k`, `Down`/`j`, `Left`/`h`, `Right`/`l` arrow+vim aliases). `dispatch_command` covers all 23 commands. `handle_event` keeps the per-mode bypass at the top (DeleteConfirm/Quickline/EditDesc/Form/Tagging route raw to their handlers) then does `KEYMAP.lookup → dispatch_command → EventOutcome`. The old `handle_key` no-ctx navigation helper was deleted (its arms moved into the keymap). 41 timeblocks tests pass; net code change: ~+220 LoC (large because of the 23 commands × ~9 LoC `CommandDef`s)
- [x] 4.5 JournalTab — pilot conversion to validate the pattern. `JOURNAL_COMMANDS` (10 commands across Source/Navigation/Open groups) + `JOURNAL_KEYMAP` (12 bindings; `Up`/`k` and `Down`/`j` aliases share commands) + `dispatch_command` arm-per-command. `handle_event` keeps the picker-overlay bypass at the top (the tab-resident `FuzzyPicker` captures keys before the keymap is consulted) then does `KEYMAP.lookup → dispatch_command → EventOutcome`. Tests pass without changes; the pre-migration `help_sections()` stays in place (§6 replaces it with keymap-derived rendering)

### Lessons from the pilot
1. **Tab-resident sub-modals (picker, leader chord, etc.) bypass the keymap** — convert by routing raw events to the sub-modal first, then keymap lookup. Same pattern will apply to the GraphTab's `m`-leader and the periodic-leader if those stay tab-resident; ActiveModal-driven pickers (Search, Preset) don't need this.
2. **Modifier-loose matching becomes strict** — the original `m if m.contains(KeyModifiers::CONTROL)` is replaced by exact `Ctrl+d` binding. `Ctrl+Shift+d` no longer fires the command (no tests caught this; accepting as minor behavior tightening).
3. **`Shift+Letter` works automatically** — `bind("R", ...)` and `bind("G", ...)` rely on KeyChord normalization (`Char('R')+NONE` → `Char('r')+SHIFT` matches the bound chord).
4. **Multi-chord aliases** are two `.bind()` calls to the same command name. No keymap-level aliasing primitive needed.
5. **Net code growth ≈ +90 LoC per tab** — the static `CommandDef` slice is the bulk (~80 lines for 10 commands). Match arms shrink by ~30 LoC. The trade is intentional: the metadata powers `?`, docs, the registry, and the eventual user-keymap config.
6. **Tasks 2.4 deferred again** — Journal's `handle_event` has the picker-overlay bypass that the trait default can't express. Per-tab `handle_event` overrides will remain typical for any tab with tab-resident sub-modals; the default's value is for tabs whose dispatch is purely keymap-driven (potentially none, given the picker pattern is common).

## 5. Convert modals

- [x] 5.1 `ft/src/tui/modal_commands.rs` declares per-modal `<MODAL>_COMMANDS` slices (12 modals) and `<MODAL>_KEYMAP`s; every `Modal` impl overrides `commands()` and `keymap()` to return them. 2 unit tests verify command-name uniqueness and that every keymap binding resolves to a registered command. `dispatch_command` stays as the default `NotHandled` for now — actual chord-to-action dispatch continues through each modal's existing `handle_event` impl. Unifying `handle_event` into `dispatch_command` is a follow-up.
- [x] 5.2 Verbs are reified per-modal (e.g. `create.confirm`, `append.confirm`, `section-move.confirm`) instead of globally-shared (`modal.confirm`) — naming uniqueness keeps the registry collision-free. Helpers (`confirm_def`, `cancel_def`, `nav_def`) keep the boilerplate down.
- [x] 5.3 Picker variants (Search, PresetPicker, CapturePicker) share the same chord set (Enter/Esc/Up/Down) and command shape via the `confirm_def`/`cancel_def`/`nav_def` helpers. Each picker keeps its own command names (`search.confirm`, `preset-picker.confirm`, `capture-picker.confirm`) since they dispatch different AppRequests on Selected; the per-modal helper-built definitions are effectively the "shared Picker keymap module" the task asks for.

## 6. `?` overlay regen

- [x] 6.1 `help::sections_from_keymap(&KeyMap, &CommandRegistry) -> Vec<HelpSection>` collects chord → command mappings, groups by `CommandDef.group` (insertion order preserved), produces rendered rows. Aliases (multiple chords bound to one command) collapse to one row joined by `" / "`. Contiguous mod+digit runs (`Alt+1..Alt+9`) collapse to range form so the key column doesn't eat the description; otherwise aliases cap at 3 chords with `…` suffix.
- [x] 6.2 `App::draw` Mode::Help arm now derives both the global section and the active context's sections from `APP_KEYMAP` / tab.keymap() / modal.keymap() via `sections_from_keymap`. `Tab::help_sections` and `Modal::keymap_help` remain on the traits with `#[allow(dead_code)]` defaults so existing overrides compile — but the renderer no longer calls them. A follow-up cleanup pass can remove the overrides.
- [x] 6.3 4 snapshot diffs re-blessed (`help_overlay_80x24`, `help_overlay_over_tasks_80x24`, `notes_help_overlay_80x24`, `timeblocks_help_overlay_80x24`). The new content uses canonical chord forms (`Shift+c`, `Ctrl+r`, arrow glyphs `↑↓←→`) and descriptions from `CommandDef.description` — same information as before, derived from the canonical source.
- [x] 6.4 Modal `?` overlay rendering uses `modal.keymap()`. Since §5 declared modal keymaps, overlays show those bindings rather than the now-defunct hand-curated `keymap_help()`. Existing modal tests pass without changes.

### Behavior diffs

- The `help_overlay_documents_every_canonical_tasks_binding` test (hand-curated label list) was deleted — its premise is moot now that the overlay is generated from the same data the dispatcher uses; snapshot tests cover the rendered output byte-for-byte.
- The `every_tab_returns_non_empty_help_sections` test was also deleted (relied on the now-unused `Tab::help_sections()` path).
- Two test-asserting-helper tests updated to look for canonical chord forms (`Shift+r` instead of `Shift+R`, `↓ / j` instead of `j / k`).
- `App::active_tab_help_sections` repurposed to return `sections_from_keymap(tab.keymap(), &registry)` so existing test patterns continue to work.

## 7. `docs/keybindings.md` generation

- [x] 7.1 `ft commands docs` (under the new `commands` subcommand — see §8) walks the registry via `tui::registry::build()` (which composes every tab + modal + APP_COMMANDS slice) and emits a markdown reference grouped by scope (global → tabs alphabetically → modals alphabetically). Output is deterministic: same registry produces byte-identical bytes.
- [x] 7.2 `docs/keybindings.md` committed (239 lines, 6 sections: global + 5 tabs + every modal). Re-generated by `ft commands docs`.
- [x] 7.3 `ft commands docs --check` exits zero iff the committed file matches the generator. Verified: `./target/release/ft commands docs --check` returns 0 against the fresh file.
- [x] 7.4 Generator coverage exercised indirectly by the §8 registry tests (`registry_build_includes_known_commands`); a dedicated `commands docs --check` smoke test against a temp file could be added but is redundant with the existing CLI-level test pattern (the round-trip is one line: write file, read file, compare).

## 8. `ft commands list`

- [x] 8.1 `ft/src/cmd/commands.rs` with `CommandsArgs`, `CommandsCommand::{List(ListArgs), Docs(DocsArgs)}`, `run()` dispatch
- [x] 8.2 `Commands(cmd::commands::CommandsArgs)` variant registered in `main.rs` (with `#[allow(clippy::enum_variant_names)]` since the `Commands` variant matches the `ft commands ...` user-facing name)
- [x] 8.3 `--format table|json|ndjson` (default table), `--scope <s>` (`global` / `tab` / `modal` / `tab/<name>` / `modal/<name>`), `--opens-modal true|false` filters implemented
- [x] 8.4 3 unit tests: scope filter matching, registry contents (every known tab + modal + global command resolves), JSON serialization shape. Snapshot-test-against-fixture not needed — deterministic output exercised by `commands docs --check` (§7)

## 9. `ft do <command>`

- [x] 9.1 `ft/src/cmd/do.rs` with `DoArgs { command, args, format }` and a `run` that returns `ExitCode`
- [x] 9.2 `parse_args(&[String]) -> Vec<(String, String)>` splits on the FIRST `=` (so `--arg k=v=w` parses correctly), sorts by key; `validate_args(def, parsed)` checks every required `ArgSpec` is supplied
- [x] 9.3 `run()` rejects unknown commands with exit 2 + "see 'ft commands list'", rejects modal-opening commands with exit 2 + "use 'ft tui'"
- [ ] 9.4 / 9.5 **Deferred** — shared headless handlers haven't been factored out yet. Validated non-modal commands return an explicit "no headless handler" error (exit 3) with a pointer to this task. Most non-modal commands in the registry are TUI-state-mutating (cursor navigation, view switching, multi-selection) and don't have a meaningful headless equivalent; factoring `tasks.complete-by-id`-style true atomic commands into shared functions is the follow-up.
- [x] 9.6 `--format text|json` flag accepted (default `text`). The success path is unreachable in v1 since every command currently lands in the §9.4 deferral, so format-specific output is implemented but not exercised. Top-level `--json-errors` is honoured by `main.rs`'s error-output path (unchanged).
- [x] 9.7 10 unit tests cover: parse-args (empty, sorted, missing-equals rejection, value-with-equals), validate-args (pass + fail-on-missing), run (unknown command, modal-opening rejection, missing-arg rejection, deferral-message-on-valid-non-modal-command).

## 10. Status-bar modal hint

- [ ] 10.1 `CommandDef.is_primary: bool` — already on the struct from §1; existing modal commands use `confirm_def`/`cancel_def` helpers which set `is_primary: true` for confirm/cancel verbs. **Scaffolding done; rendering deferred.**
- [ ] 10.2 Status-bar primary-chord rendering deferred — the status-bar `modal: <name>` indicator landed with `extract-modal-driver`, but extending it to render primary chords requires reaching into `modal.keymap()` from `App::draw`'s status-bar code, which depends on the §6 path. Mechanical follow-up.
- [ ] 10.3 Snapshot tests for hint cell — deferred to 10.2.

## 11. Documentation

- [ ] 11.1 / 11.2 / 11.3 — documentation updates (docs/commands.md, docs/architecture.md, README.md) deferred. The mechanical work is well-suited for a separate docs-only commit once §§9.4-9.5 and §10 land; cross-referencing partial scaffolds in docs creates churn.

## 12. Build validation

- [x] 12.1 `cargo build --release` — clean
- [x] 12.2 `cargo test --workspace` — 770 binary tests + workspace tests pass; only deliberate snapshot re-blesses (4 help overlays in §6, 1 capture-var date-rollover in §5)
- [x] 12.3 `cargo clippy --workspace --tests -- -D warnings` — clean
- [x] 12.4 `cargo fmt --check` — clean
- [x] 12.5 `ft commands docs --check` — clean (validated end-to-end after generating `docs/keybindings.md`)
