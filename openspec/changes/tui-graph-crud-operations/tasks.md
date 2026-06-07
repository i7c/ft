## 1. Core delete functions (ft-core)

- [x] 1.1 Add `fs::delete_file(path: &Path) -> Result<()>` — wraps `std::fs::remove_file`, returns `Error::Io` on failure, succeeds silently if the file doesn't exist
- [x] 1.2 Add `fs::delete_directory(path: &Path) -> Result<()>` — wraps `std::fs::remove_dir_all`, validates path is not the vault root, returns `Error::Io` on failure
- [x] 1.3 Add `graph::delete` module with `DeletePlan` struct (holds `Vec<PathBuf>` of paths to delete) and `plan_delete(path: &Path, vault_root: &Path) -> Result<DeletePlan>` — validates the path is under vault root, adds it to the plan
- [x] 1.4 Add `apply_delete(plan: &DeletePlan) -> Result<()>` — iterates plan entries, calls `fs::delete_file` or `fs::delete_directory` based on whether the path is a file or directory, then cleans up empty parent directories bottom-up (reuse pattern from `graph::rename`)
- [x] 1.5 Unit tests for `delete_file`, `delete_directory`, `plan_delete`, `apply_delete` with `assert_fs` temp dirs

## 2. CLI delete command

- [x] 2.1 Add `Commands::Graph(GraphCommands::Delete { path })` variant in `ft/src/main.rs`
- [x] 2.2 Wire `ft graph delete <path>` to `plan_delete` + `apply_delete` with vault discovery and `--vault`/`FT_VAULT` support
- [x] 2.3 Print confirmation message on success; error message + non-zero exit on failure
- [x] 2.4 Integration test with `assert_cmd` + `assert_fs` fixture vault

## 3. ConfirmDelete modal (TUI)

- [x] 3.1 Add `ActiveModal::ConfirmDelete(ConfirmDeleteState)` variant to the `ActiveModal` enum in `ft/src/tui/modal.rs`
- [x] 3.2 Define `ConfirmDeleteState` struct with fields: `message: String` (the confirmation question), `target: PathBuf` (vault-relative path of item to delete), `is_directory: bool`, `focus: ConfirmChoice { Yes, No }`
- [x] 3.3 Implement `Modal` trait for `ConfirmDeleteState`:
  - `handle_event`: y→select Yes, n/Esc/q→select No, Left/h and Right/l→cycle focus, Enter→commit focused choice, everything else→Consumed
  - `render`: centered bordered box with the message and `[Yes]`/`[No]` buttons, highlighted per focus
  - `name`: return `"confirm-delete"`
  - `commands`: return `CONFIRM_DELETE_COMMANDS` slice
  - `keymap`: return `&CONFIRM_DELETE_KEYMAP`
- [x] 3.4 Add `CONFIRM_DELETE_COMMANDS` and `CONFIRM_DELETE_KEYMAP` to `ft/src/tui/modal_commands.rs` (commands: `confirm-delete.yes`, `confirm-delete.no`; keys: y/n/Esc/q/Enter/Left/Right/h/l)
- [x] 3.5 Wire `ActiveModal::ConfirmDelete` into the `Modal` impl dispatch in `modal.rs` (handle_event, render, commands, keymap, name)
- [x] 3.6 In `graph.rs` `dispatch_command`, add `"graph.delete"` arm: on Note/Directory rows, post `OpenModal(ActiveModal::ConfirmDelete(...))`; on Ghost/Task, post error toast; on empty tree, do nothing
- [x] 3.7 In `graph.rs`, add commit-delete hook: when `ConfirmDelete` closes with Yes, call `plan_delete` + `apply_delete`, refresh graph, show success toast
- [x] 3.8 Snapshot tests for the ConfirmDelete modal rendering (via `TestBackend`)

## 4. Ghost instant creation (TUI)

- [x] 4.1 In `graph.rs` `dispatch_command` for `"graph.create-blank"`: check if focused row is Ghost; if so, compute abs path from `g.raw`, create parent dirs if needed, write `# <title>\n` via `write_atomic`, open in editor, refresh graph, return Handled (no modal)
- [x] 4.2 In `graph.rs` `dispatch_command` for `"graph.create-from-template"`: check if focused row is Ghost; if so, open `ActiveModal::Create(CreateState::TemplatePicking { folder_seed: Some(ghost_parent_dir) })` — the template picker commits to the ghost's resolved path (modify `commit_create` or pass the target path through the create flow)
- [x] 4.3 Ensure non-ghost rows (Note, Directory, Task, Paragraph) still get the existing create flow behavior unchanged
- [x] 4.4 Integration tests for ghost creation: set up fixture vault with a ghost node, trigger create, verify file appears

## 5. CreateSubdir modal (TUI)

- [x] 5.1 Add `ActiveModal::CreateSubdir(CreateSubdirState)` variant to the `ActiveModal` enum
- [x] 5.2 Define `CreateSubdirState` struct with fields: `parent: PathBuf` (vault-relative parent directory), `buf: EditBuffer` (subfolder name)
- [x] 5.3 Implement `Modal` trait for `CreateSubdirState`:
  - `handle_event`: Esc→Closed, Enter→validate (non-empty, no `/`, target not existing)→create via `std::fs::create_dir_all`, post refresh + toast + Closed; validation errors set error text and Stay; printable chars/Backspace/etc. route to `EditBuffer`
  - `render`: bordered box with title "Create subdirectory in `<parent>`", input field with cursor, error text if any
  - `name`: return `"create-subdir"`
  - `commands`: return `CREATE_SUBDIR_COMMANDS` slice
  - `keymap`: return `&CREATE_SUBDIR_KEYMAP`
- [x] 5.4 Add `CREATE_SUBDIR_COMMANDS` and `CREATE_SUBDIR_KEYMAP` to `modal_commands.rs` (commands: `create-subdir.confirm`, `create-subdir.cancel`; keys: Enter→confirm, Esc→cancel)
- [x] 5.5 Wire `ActiveModal::CreateSubdir` into the `Modal` impl dispatch
- [x] 5.6 In `graph.rs` `dispatch_command`, add `"graph.create-subdir"` arm: on Directory rows, derive parent path from `create_folder_from_selection`, post `OpenModal(ActiveModal::CreateSubdir(...))`; on other rows, post error toast
- [x] 5.7 Snapshot tests for the CreateSubdir modal rendering

## 6. Key bindings and command registration

- [x] 6.1 Add `graph.delete` and `graph.create-subdir` `CommandDef` entries to `GRAPH_COMMANDS` in `graph.rs` (group "Notes", opens_modal: true for both)
- [x] 6.2 Add `.bind("d", "graph.delete")` and `.bind("n", "graph.create-subdir")` to `GRAPH_KEYMAP`
- [x] 6.3 Regenerate `docs/keybindings.md` if CI requires it

## 7. Final integration and polish

- [x] 7.1 Smoke-test all three operations in the TUI with a real vault: delete note, delete directory, create ghost with c, create ghost with C, create subdirectory
- [x] 7.2 Run `cargo build --release`, `cargo test --workspace`, `cargo clippy --workspace --tests -- -D warnings`, `cargo fmt --check`
- [x] 7.3 Review snapshot diffs and re-bless any expected changes
