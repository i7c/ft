## 1. Tab trait + App plumbing

- [x] 1.1 Add `queue_journal_for(&mut self, note_path: &Path)` default-no-op method to the `Tab` trait in `ft/src/tui/tab.rs` (mirrors `queue_related_modal`)
- [x] 1.2 Add `AppRequest::JournalForNote { path: PathBuf }` variant in `ft/src/tui/tab.rs`
- [x] 1.3 Service `AppRequest::JournalForNote` in `App::service_request` (or wherever pending requests are dispatched): call `queue_journal_for(&path)` on the Journal tab and switch the active tab index

## 2. Journal tab (ft/src/tui/tabs/journal.rs)

- [x] 2.1 Create `JournalTab` struct with fields: `target_path: Option<PathBuf>`, `entries: Vec<JournalEntry>`, `selected: usize`, `scroll_offset: usize`, `picker: Option<FuzzyPicker<VaultFilePickerSource>>`, `queued_for: Option<PathBuf>`, `cache: Option<BlameCache>`, `last_error: Option<String>`
- [x] 2.2 Implement `Tab::title() -> "Journal"` and `Tab::on_focus` — consume `queued_for` if set and run a load
- [x] 2.3 Implement `Tab::queue_journal_for` to store the path in `queued_for`
- [x] 2.4 Implement `load_for(path: &Path, ctx)` helper: resolve `discover_repo`, ensure `cache` is loaded, call `ft_core::journal::build_journal`, replace `entries`, reset `selected`/`scroll_offset`, best-effort save the cache, set `last_error` on failure
- [x] 2.5 Implement `Tab::handle_event` keymap: `/` opens picker (if none); picker dispatch; `j`/`k`/`Up`/`Down` move selection; `Ctrl+D`/`Ctrl+U` half-page; `g`/`G` first/last; `R` reloads when `target_path` is set; `c` clears state back to empty; `Enter` raises `AppRequest::OpenEditor` (or equivalent existing variant) at the selected entry's `source_path` + paragraph `line_start`
- [x] 2.6 Implement `Tab::render`: empty-state prompt when `target_path.is_none()` and no error; loaded-state list of entries (date, source title, paragraph text, separator), with the selected entry visually highlighted; picker overlay rendered last when active; `last_error` shown as a small banner if set
- [x] 2.7 Implement `Tab::help_sections` covering Navigation, Source picker, Reload/clear, Open-in-editor groups
- [x] 2.8 Register the new module in `ft/src/tui/tabs/mod.rs`
- [x] 2.9 Insert `Box::new(JournalTab::new())` into the `tabs` vector in `App::with_tabs` / `App::new_with_recents` (after the Graph tab)

## 3. Graph-tab jump

- [x] 3.1 Add `Shift+J` (`KeyCode::Char('J')` with `KeyModifiers::SHIFT`) handling in `ft/src/tui/tabs/graph.rs` `handle_event`. When the selected row is a `NodeKind::Note`, raise `AppRequest::JournalForNote { path: note.path.clone() }`. When the selected row is non-Note or none, queue an informational toast instead.
- [x] 3.2 Add a help-overlay entry on the graph tab for `Shift+J: open Journal for selected note` (extend an existing `HelpSection` or add a new "Cross-tab" section)

## 4. Tests (TestBackend snapshots + behavior)

- [x] 4.1 Snapshot test: Journal tab empty-state (`switch_to_journal`, render 80×24) shows the picker prompt
- [x] 4.2 Behavior test: simulate selecting a note via `queue_journal_for` + focus, then render — assert the entries list contains the expected date/title/paragraph text
- [x] 4.3 Help-overlay test: assert the Journal tab's `help_sections` includes `/`, `R`, `c`, `Enter`, and navigation bindings
- [x] 4.4 Behavior test: graph-tab `Shift+J` with a Note row selected raises `AppRequest::JournalForNote { path }` and switches the active tab to Journal
- [x] 4.5 Behavior test: graph-tab `Shift+J` with no Note selected queues an error toast and does NOT switch tabs
- [x] 4.6 Help-overlay test: graph tab's `help_sections` includes the `Shift+J` row

## 5. Build invariants

- [x] 5.1 `cargo build --release` clean
- [x] 5.2 `cargo test --workspace` all green
- [x] 5.3 `cargo clippy --workspace --tests -- -D warnings` clean
- [x] 5.4 `cargo fmt --check` clean
