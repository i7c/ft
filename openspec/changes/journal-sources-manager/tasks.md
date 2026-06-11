## 1. Tab trait + AppRequest plumbing

- [ ] 1.1 Add `AppendOrReplaceMode` enum (`Append`, `Replace`) to `ft/src/tui/tab.rs` (or a small new module if it grows)
- [ ] 1.2 Add `AppRequest::JournalAddSources { targets: Vec<JournalTarget>, default_mode: AppendOrReplaceMode }` variant and corresponding manual `Debug` arm
- [ ] 1.3 Add `Tab::queue_journal_add_sources(&mut self, _targets: Vec<JournalTarget>, _default_mode: AppendOrReplaceMode)` default no-op hook
- [ ] 1.4 Add `AppRequest::JournalCommitSources { sources: Vec<JournalTarget>, window: Option<JournalWindow> }` for the manager modal commit path; manual Debug arm
- [ ] 1.5 Add `Tab::queue_journal_commit_sources(&mut self, _sources: Vec<JournalTarget>, _window: Option<JournalWindow>)` default no-op hook

## 2. Ghost-aware picker source

- [ ] 2.1 Add `JournalSourceHit` enum (`Note(PathBuf)`, `Ghost(String)`) in `ft/src/tui/widgets/picker.rs` (or a sibling module)
- [ ] 2.2 Add `JournalSourcePickerSource` struct holding `Arc<Vault>`, `Arc<RecentsLog>`, a path matcher, and an owned `Graph` snapshot
- [ ] 2.3 Implement `PickerSource for JournalSourcePickerSource`: `query()` merges `fuzzy_find` real-note hits with fuzzy-matched ghost names from `graph.nodes()` into one ranked result list; `initial_items()` returns recent-notes hits only (ghosts must be searched for explicitly)
- [ ] 2.4 Unit tests in `picker.rs::tests` mod: ghost-only input returns ghost rows; mixed input ranks notes and ghosts in one list; selection returns the right `JournalSourceHit` variant
- [ ] 2.5 Update label rendering helpers to suffix ` (ghost)` on ghost rows

## 3. Sources Manager modal

- [ ] 3.1 Add `ActiveModal::JournalSources(JournalSourcesModal)` variant in `ft/src/tui/modal.rs` (and `Modal::name()` -> `"journal-sources"`)
- [ ] 3.2 Implement `JournalSourcesModal` state: `sources: Vec<JournalTarget>`, `cursor: usize`, `picker: Option<FuzzyPicker<JournalSourcePickerSource>>`, `window: Option<JournalWindow>`
- [ ] 3.3 Implement modal keymap: `j`/`k`/`Up`/`Down` row nav; `d` remove focused; `c` clear all; `a` open inner add-source picker; `Enter` commit (raises `JournalCommitSources`); `Esc` cancel (still rebuilds with current sources)
- [ ] 3.4 Inner picker key handling: `Esc` returns to row list, `Enter` appends the selected `JournalSourceHit` to `sources` (deduplicate by `JournalTarget == ` equality) and returns to row list
- [ ] 3.5 Render the modal as a centered popup: titled block `Journal Sources — N source(s)`; row list above; footer key cue `a add  d remove  c clear  Enter commit  Esc cancel`; when picker is active, render it inside the modal area
- [ ] 3.6 App-side wiring in `app.rs`: dispatch `JournalCommitSources` to the Journal tab's `queue_journal_commit_sources` hook

## 4. Append/Replace prompt modal

- [ ] 4.1 Add `ActiveModal::JournalAppendOrReplace { incoming_targets: Vec<JournalTarget>, focus: AppendOrReplaceMode }` variant (and `Modal::name()` -> `"journal-append-or-replace"`)
- [ ] 4.2 Implement keymap: `a`/`A` commits Append; `r`/`R` commits Replace; `c`/`C`/`Esc` cancel; `Left`/`Right`/`Tab` flip the focused choice; `Enter` commits the focused choice
- [ ] 4.3 Render at the bottom of the tab area (5 rows tall, mirroring the `NonSynthPrompt` shape): titled block with `Append N target(s) to current sources?`; choice line `[a] append   [r] replace   [c] cancel`; selected choice highlighted with `BOLD | REVERSED`
- [ ] 4.4 Commit dispatches a new `AppRequest::JournalCommitSources` carrying either the union (append) or replacement (replace) source set

## 5. Journal tab refactor: single `sources` slot

- [ ] 5.1 Replace `target: Option<JournalTarget>` and `multi_targets: Vec<JournalTarget>` on `JournalTab` with `sources: Vec<JournalTarget>`; keep `window: Option<JournalWindow>` and `in_window_only: bool`
- [ ] 5.2 Remove the tab-resident `picker: Option<FuzzyPicker<VaultFilePickerSource>>` field and its `handle_picker_key` early-return in `handle_event`
- [ ] 5.3 Implement `rebuild_journal(&mut self, ctx: &mut TabCtx)` that: bails with a banner if no git repo; bails with a banner if `sources.is_empty()` (but does NOT show error — just shows empty entry list); builds a fresh `Graph`; resolves each source to a `NoteId`; calls `build_journal(graph, &ids, vault, &vault_path, cache)`; updates `entries`, `entry_matched_titles`, `entry_selected`, `selected = 0`, `scroll_offset = 0`; preserves `in_window_only` only if `sources.len() >= 2 && window.is_some()`, else clears it
- [ ] 5.4 Replace `load_for(target)` callers with: `self.sources = vec![target]; self.window = None; self.in_window_only = false; self.rebuild_journal(ctx)`
- [ ] 5.5 Replace `load_for_multi(request)` callers with: `self.sources = request.targets; self.window = request.window; self.in_window_only = false; self.rebuild_journal(ctx)`
- [ ] 5.6 Add `queued_add_sources: Option<(Vec<JournalTarget>, AppendOrReplaceMode)>` slot; override `queue_journal_add_sources` to fill it
- [ ] 5.7 Add `queued_commit_sources: Option<(Vec<JournalTarget>, Option<JournalWindow>)>` slot; override `queue_journal_commit_sources` to fill it
- [ ] 5.8 In `on_focus`, consume queues in priority order: commit_sources > multi > single > add_sources (add_sources turns into raising the Append/Replace prompt); rebuild after each
- [ ] 5.9 Delete `load_for` and `load_for_multi` functions

## 6. Sources strip rendering

- [ ] 6.1 Add `render_sources_strip(frame, area, sources, window, in_window_only)` helper rendering exactly 2 rows
- [ ] 6.2 Line 1: `Sources (N)` plus `[window: <label>]` when `window.is_some()` plus `[filter: in-window]` when `in_window_only`
- [ ] 6.3 Line 2: comma-joined source labels (Note path or `<raw> (ghost)`); truncate with `…, +K more` when wider than inner width
- [ ] 6.4 In `JournalTab::render`, split the inner area into `[strip(2), entries(rest)]` and call the strip helper before the entry list
- [ ] 6.5 Empty-state path renders the strip plus an empty entry-list area (no centered prompt)
- [ ] 6.6 Update entry-list scroll math so `view_height` reflects the reduced inner area (post-strip)

## 7. Journal tab keymap & commands

- [ ] 7.1 Replace `journal.open-picker` command/keymap entry with `journal.open-sources-manager`; bind to `/` and add `+` as an alias
- [ ] 7.2 Update `journal.clear` to call the same path the manager's `c` action uses (sources cleared + rebuild)
- [ ] 7.3 Implement `open_sources_manager(&mut self, ctx)`: build a fresh `Graph` snapshot, construct the `JournalSourcePickerSource` and `JournalSourcesModal` (default landed on the add-source picker), raise `AppRequest::OpenModal(...)`
- [ ] 7.4 Update `help_sections()` for the Journal tab: rename "Source" group to "Sources"; entries for `/`, `+`, `c`, `R`, plus the existing entry-nav and open/synth entries

## 8. Graph tab append command

- [ ] 8.1 Add `graph.add-to-journal-sources` `CommandDef` and bind it to `Shift+A` in `GRAPH_KEYMAP`
- [ ] 8.2 Implement the handler: read `multi_selected` (or cursor row if empty); resolve each `NodeKey` to `NodeKind`; map Notes to `JournalTarget::Note(path)`, Ghosts to `JournalTarget::Ghost(raw)`; silently skip Directory/Task/Paragraph; if zero targets resolve, toast `"no Note or Ghost rows selected"`; otherwise raise `AppRequest::JournalAddSources { targets, default_mode: Append }`
- [ ] 8.3 Update Graph tab's `help_sections()` to list `Shift+A: append selected (or cursor) to Journal sources`

## 9. App-side request dispatch

- [ ] 9.1 In `App::service_request`, handle `JournalAddSources` by switching to the Journal tab and calling `queue_journal_add_sources(targets, default_mode)`
- [ ] 9.2 Handle `JournalCommitSources` by routing to the Journal tab's `queue_journal_commit_sources(sources, window)`
- [ ] 9.3 Handle `OpenModal(JournalSources)` and `OpenModal(JournalAppendOrReplace)` via the existing modal-driver code path

## 10. Tests

- [ ] 10.1 Snapshot test: empty Journal tab — strip shows `Sources (0)` + hint
- [ ] 10.2 Snapshot test: Journal tab with 1 source (Note) — strip + entries
- [ ] 10.3 Snapshot test: Journal tab with 3 sources (mixed Note + Ghost) + attached window — strip lists all three, shows `[window: since 7d]`
- [ ] 10.4 Snapshot test: Journal tab with many sources, narrow terminal — strip truncates to `…, +K more`
- [ ] 10.5 Snapshot test: Sources Manager modal open with 3 rows + footer key cue
- [ ] 10.6 Snapshot test: Sources Manager modal with inner add-source picker active showing one Note and one Ghost row
- [ ] 10.7 Snapshot test: Append/Replace prompt rendered at bottom with focused choice
- [ ] 10.8 Integration test in `ft/tests/`: build a temp vault with two notes + one ghost reference, open the TUI, multi-select two notes on Graph, press `Shift+A`, choose `append` on the prompt, assert the Journal sources strip lists the union
- [ ] 10.9 Integration test: Review-tab handoff still works (existing behavior preserved)
- [ ] 10.10 Integration test: Graph `Shift+J` single-target jump still works on both Note and Ghost rows
- [ ] 10.11 Property check (or table-driven test) on the dedup invariant of the append commit path

## 11. Build invariants

- [ ] 11.1 `cargo build --release` clean
- [ ] 11.2 `cargo test --workspace` green (after accepting new insta snapshots)
- [ ] 11.3 `cargo clippy --workspace --tests -- -D warnings` clean
- [ ] 11.4 `cargo fmt --check` clean
