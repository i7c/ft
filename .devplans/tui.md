---
id: 002
name: tui
title: Interactive TUI for vault management
status: proposed
created: 2026-05-09
updated: 2026-05-09
---

# Interactive TUI for vault management

## Goal
Add an interactive `ft tui` subcommand that opens a tabbed full-screen terminal
UI built on `ratatui`. The first iteration ships a Tasks tab — list, filter,
sort, complete, edit, create — with an explicit, well-handled story for stale
views (the user edits files in Obsidian while the TUI is open). Tabs for notes
and other vault content are out of scope and live in plan 003.

## Motivation and Context
The CLI from plan 001 is great for scripting and quick lookups, but for daily
"what should I work on now?" workflows the user wants to scan, filter, and
update tasks without re-typing flags. A persistent TUI also makes bulk
operations (multi-select, bulk-move, bulk-priority-bump) much more ergonomic.
Critically, the TUI must not silently drift from disk: if the user edits a
note in Obsidian while a TUI view is open, we need to detect it and either
auto-refresh or surface a clear "view is stale, press R to refresh" affordance.
The user explicitly raised this as a concern, so it's a load-bearing
requirement, not a nice-to-have.

## Acceptance Criteria

### Foundation
- [ ] `ft tui` subcommand launches the TUI; exits cleanly on `q` or Ctrl+C
- [ ] Single binary (no separate `ft-tui`); the subcommand is registered alongside the others in plan 001
- [ ] Renders correctly in 80x24 minimum, scales gracefully up to large terminals
- [ ] Mouse support: optional, off by default (configurable in `.ft/config.toml`)
- [ ] Theme: dark-first; light-mode auto-detect via `COLORFGBG` if available; explicit `--theme` flag overrides
- [ ] All keybindings discoverable via `?` help overlay; bindings are configurable in `.ft/config.toml`
- [ ] `ft tui` reuses the same vault discovery and config as the CLI from plan 001

### Tab system
- [ ] Top bar shows tabs; switch with `1..9` and `Tab`/`Shift-Tab`
- [ ] Tabs in v1: only "Tasks". The framework supports adding more tabs without code surgery (one trait `Tab { render, handle_event, on_focus, on_blur, refresh }`)
- [ ] Status bar at bottom shows: vault name, current tab, last-refresh time, modified-on-disk indicator, mode hint

### Tasks tab — list view
- [ ] Loads tasks on tab focus (lazy: not loaded until user opens the tab)
- [ ] Sortable columns: status, priority, due, scheduled, description, path
- [ ] Filter bar at top: free-text fuzzy filter on description + a `/` slash command that opens the full query DSL
- [ ] Live preset selector: `Today`, `Overdue`, `Upcoming`, `All Open`, plus user-defined presets from config
- [ ] Multi-select with `Space`; visible count of selected items in status bar
- [ ] `Enter` opens the source note in `$EDITOR` at the right line
- [ ] `c` completes selected tasks (handles recurrence per plan 001 rules)
- [ ] `e` edits the selected task in-place via a popup form (description, dates, priority, tags); writes back atomically
- [ ] `m` opens move dialog for selected tasks: target file via fuzzy file picker, optional heading
- [ ] `n` opens create dialog (see below)
- [ ] `g` group-by cycle (path, due, priority, tag, none)
- [ ] `s` sort cycle; `S` reverses
- [ ] `R` forces refresh from disk

### Stale view detection
- [ ] On tab focus, check mtime on each file that contributed to the current view; if any changed since last load, mark view as stale
- [ ] Stale state is visually obvious: yellow banner at top of the list area saying "X files changed on disk since last load — press R to refresh" with the file count
- [ ] Optional auto-refresh on focus (config: `tui.tasks.auto_refresh_on_focus = true|false`, default false)
- [ ] If the user attempts a mutation (`c`, `e`, `m`) on a stale row whose source line no longer matches what we cached, we **refuse the operation** and show a modal explaining the file changed; offer "Refresh and retry" or "Cancel"
- [ ] Optional inotify/FSEvents watcher (feature-flagged via `notify` crate) that updates the staleness indicator in near-real-time without polling. Default off if it costs significant battery; document the tradeoff
- [ ] Background polling fallback at 5s interval when watcher is disabled; only checks files in the current view

### Tasks tab — create dialog
- [ ] Triggered by `n`. Modal form with fields: description (required), due, scheduled, priority, tags, recurrence
- [ ] Target location selector at top of the form, defaulting to the rules from plan 001 (today's daily note unless context says otherwise)
- [ ] Context-aware default: if the TUI was launched with `ft tui --in <file>` or the user is currently filtered to a single file, the create dialog defaults the target to that file
- [ ] Bulk-create mode (toggle in dialog): enter multiple descriptions, one per line, and a single shared target dropdown; on submit, all are written in one atomic batch per file
- [ ] Date inputs accept ISO, relative, natural language (same parser as CLI)
- [ ] `Esc` cancels; `Ctrl+S` or "Submit" button writes; "Submit & New" keeps the dialog open with the same target

### Mutation safety
- [ ] All file writes go through the same `ft-core` atomic write helper from plan 001
- [ ] Every mutation produces an in-memory undo entry; `u` undoes the last action (limited to current TUI session). Undo replays the inverse mutation, not just an in-memory state revert — disk and memory must agree
- [ ] If the underlying file has changed since the cached version when undoing, refuse and tell the user to refresh first

### Performance
- [ ] First render of the tasks list under 500ms on a 5k-note vault (matches the CLI scan target from plan 001 since it's the same scan path)
- [ ] Filter typing remains responsive (under 50ms per keystroke) on the same vault — implies in-memory filter, not re-scan
- [ ] Memory ceiling: under 200MB for the 5k-note vault baseline

### Testing
- [ ] Unit tests on the tab framework's event dispatch and state transitions
- [ ] Snapshot tests for rendered frames using `ratatui`'s `TestBackend` — at least: empty tasks list, populated tasks list, stale banner visible, create dialog open, error modal, help overlay
- [ ] Integration tests that drive the TUI with a scripted event stream against fixture vaults from plan 001
- [ ] One end-to-end test that mutates a fixture vault file mid-test (from outside the TUI) and asserts the stale banner appears
- [ ] Manual test checklist in `docs/tui-manual-tests.md` for things automated tests can't easily cover (color, resize, mouse)

### Documentation
- [ ] `docs/tui.md` — keybinding reference, configuration, screenshots
- [ ] In-app help overlay (`?`) generated from the same source-of-truth as the docs (avoid drift)

## Technical Notes

### Library boundaries
The TUI depends only on `ft-core`. It does NOT call `ft` (the binary)
internally. Anything the TUI needs that doesn't exist in `ft-core` gets added
to `ft-core` first, and the CLI gets to benefit too.

### Architecture
A single `App` struct holds the tab list, current tab index, global state
(vault handle, config, undo stack), and a message channel. Events from
crossterm are translated to a typed `Event` enum and dispatched to the focused
tab. Tabs implement a `Tab` trait so adding new ones is a self-contained file.

```rust
trait Tab {
    fn title(&self) -> &str;
    fn on_focus(&mut self, ctx: &mut TabCtx) -> Result<()>;
    fn on_blur(&mut self, ctx: &mut TabCtx) -> Result<()>;
    fn handle_event(&mut self, ev: Event, ctx: &mut TabCtx) -> Result<EventOutcome>;
    fn render(&mut self, frame: &mut Frame, area: Rect, ctx: &TabCtx);
    fn refresh(&mut self, ctx: &mut TabCtx) -> Result<()>;
}
```

`TabCtx` carries vault, config, theme, status-bar setters, and the undo stack.

### Stale view detection — implementation sketch
On scan, we record per-file `(path, mtime, len)`. On focus or on a 5s tick, we
re-stat the same files. If any tuple differs, we set `view_stale = true` and
remember which files changed for a precise banner message. Inotify/FSEvents
via the `notify` crate is an enhancement that flips the same flag faster; the
polling fallback is the correctness baseline.

For mutations we go further: we re-read the source line at `(file, line)` and
compare it to the cached source line. If they differ, the row is stale even if
the file mtime check missed it (e.g. the user edited the file twice and the
second edit happened to revert mtime — paranoid but cheap). Refuse the
mutation and surface the modal.

### Editor handoff
Suspend the TUI (`disable_raw_mode`, leave alternate screen), spawn `$EDITOR`
synchronously, restore on return, then **force a refresh** of the current view
since we just told the user to edit something. This is the same primitive as
`ft tasks create --edit` from plan 001 but wrapped in TUI suspend/restore.

### Out of scope for this plan
- Notes tab (plan 003)
- Vault graph / link visualization
- Plugin compatibility beyond the Tasks plugin
- Web UI / remote access — strictly local TUI
- Sync conflict UX beyond the stale-view modal

## Sessions
