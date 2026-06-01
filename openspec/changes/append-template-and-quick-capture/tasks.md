## 1. Core: append-template module in ft-core

- [x] 1.1 Create `ft-core/src/notes/append.rs` with `append_template(file_content, rendered_template, section_heading) -> Result<(String, usize)>` — pure function that appends to end of file or after a named section
- [x] 1.2 Implement section-end detection via `extract_headings` — case-insensitive trimmed match, any ATX level, first match wins
- [x] 1.3 Implement frontmatter parsing: extract YAML frontmatter block from file content, deserialize `ft-append-section` key (use `serde_yaml`; already a dependency)
- [x] 1.4 Add public `frontmatter_append_section(content: &str) -> Option<String>` helper for callers that need the frontmatter value without doing the append
- [x] 1.5 Register `pub mod append;` in `ft-core/src/notes.rs`
- [x] 1.6 Add unit tests for append-to-end, append-to-section, section-not-found, empty file, missing frontmatter, frontmatter present, trailing-newline edge cases

## 2. Config: capture presets in ft-core

- [x] 2.1 Define `CapturePreset` struct in `ft-core/src/config.rs` (fields: `action: CaptureAction`, `template: String`, `note: Option<String>`, `section: Option<String>`, `path: Option<String>`, `folder: Option<String>`)
- [x] 2.2 Define `CaptureAction` enum (`Append`, `Create`) with serde deserialization from lowercase strings
- [x] 2.3 Add `capture_presets: HashMap<String, CapturePreset>` to `Config` struct, default empty map, `#[serde(default)]`, `#[serde(deny_unknown_fields)]`
- [x] 2.4 Add unit tests for config serialization round-trip, unknown key rejection, missing required fields, invalid action

## 3. CLI: `ft notes append` subcommand

- [x] 3.1 Add `Append(AppendArgs)` variant to `NotesCommand` in `ft/src/cmd/notes.rs`
- [x] 3.2 Define `AppendArgs` struct: `path` (required target), `--template` (required), `--section` (optional override), `--title` (optional), `--var` (repeatable KEY=VAL), `--no-open`, `--editor`, `--obsidian`, `--vault-name`
- [x] 3.3 Implement `run_append` — resolve target path, read file, resolve and render template, call `append_template` from core, `write_atomic`, spawn editor at insertion line
- [x] 3.4 Wire `NotesCommand::Append` dispatch in `run()` match
- [ ] 3.5 Add CLI integration tests in `ft/tests/` (create fixture vault with notes + frontmatter, run append commands, assert file contents and editor invocation)

## 4. TUI: append flow state machine

- [x] 4.1 Add `AppendState` enum to `ft/src/tui/notes_actions/` (new module `append.rs` or extend `create.rs`): states for `TemplatePicking` → `commit_append`; skip folder/filename since target already exists
- [x] 4.2 Implement `commit_append` helper — read target file content, read frontmatter for `ft-append-section` (or use explicit override), call `append_template` from core, `write_atomic`, queue `OpenInEditor` with computed line number
- [x] 4.3 Register `pub mod append;` in `ft/src/tui/notes_actions/mod.rs`

## 5. TUI: append keybindings

- [x] 5.1 Graph tab: add `A` (shift-a) handler — if selected node is a note, open template picker with target = selected note; on template selection, call `commit_append`; if selection is not a note, queue error toast
- [x] 5.2 Graph tab `create_state` slot: generalize to `opt_create_or_append` (or add separate `append_state: Option<AppendState>`); add `AppendPicking` variant to the graph tab's create_state dispatch
- [x] 5.3 Notes tab: add `a` handler in `handle_idle_key` — open template picker, then vault file picker for target note, then `commit_append`
- [x] 5.4 Add help entries for `a`/`A` in both tabs' `help_sections()` output

## 6. TUI: quick capture preset model and picker

- [x] 6.1 Add `CapturePresetPickerSource` (implements `PickerSource`) listing preset names from `ctx.vault.config.config.capture_presets`
- [x] 6.2 Implement `execute_capture_preset(ctx, preset_name)` — looks up the preset, resolves target, renders template, calls `commit_create` or `commit_append` as appropriate
- [x] 6.3 For append presets with no `note`: from graph tab use selected note, from notes tab open vault file picker → then append
- [x] 6.4 For create presets with no `path`: open filename prompt (reuse `EditBuffer` pattern from create flow), then create
- [x] 6.5 For create presets with `path`: resolve strftime tokens via `chrono::NaiveDate::format`, combine with `folder`, create file (overwrite on collision)

## 7. TUI: quick capture keybindings

- [x] 7.1 Graph tab: add `Q` (shift-q) handler in idle state — open `CapturePresetPickerSource` in a `FuzzyPicker`; on selection, call `execute_capture_preset`
- [x] 7.2 Notes tab: add `Q` handler in idle state — same preset picker flow; for append presets without `note`, open vault file picker after preset selection
- [x] 7.3 Add `QuickCapturePicking` state variant to both tabs' state enums
- [x] 7.4 Add help entries for `Q` in both tabs' `help_sections()` output

## 8. Integration tests

- [x] 8.1 Add TUI snapshot tests for append flow states (template picker, commit result) using `TestBackend`
- [x] 8.2 Add TUI snapshot tests for quick capture preset picker
- [x] 8.3 Add integration tests for append with frontmatter section targeting
- [x] 8.4 Add integration tests for quick capture preset execution (append + create variants)

## 9. Build verification

- [x] 9.1 Run `cargo build --release` — must pass
- [x] 9.2 Run `cargo test --workspace` — must pass
- [x] 9.3 Run `cargo clippy --workspace --tests -- -D warnings` — must pass
- [x] 9.4 Run `cargo fmt --check` — must pass
