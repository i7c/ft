---
id: 022
name: help-per-tab
title: TUI help: per-tab keybindings + config-extensible design
status: finished
created: 2026-05-25
updated: 2026-05-25
---

# TUI help: per-tab keybindings + config-extensible design

## Goal
Make the `?` help overlay show the keybindings that are actually live on the
active tab, instead of the current static list. Lay groundwork so that a later
plan can drive both dispatch and help from a single config-overridable keymap.

## Motivation and Context
Today the `?` overlay renders a single static `HELP_LINES` constant in
`ft/src/tui/ui.rs:206-227`. That list is a leak of Tasks-tab bindings (priority
cycle, due/scheduled nudges, complete/cancel, quickline, edit popup, …) and is
misleading on Graph, Notes, and Timeblocks. After plan 021 the Graph tab in
particular has a rich, leader-driven keymap (`p` periodic, `m` move, `Ctrl+N`
new view, …) that the user has no in-app way to discover.

Each tab already owns its own dispatch, so the natural fit is for each tab to
also own its help content. While we're here we should not paint ourselves into
a corner: a future plan wants config-driven keybindings, so the help model
should be a shape that a `KeyMap`-driven dispatcher can fill in later without
re-touching every tab.

## Acceptance Criteria
- [x] Opening `?` on each of the four tabs shows the bindings that actually
      work on that tab, plus a shared "global" section (quit, help toggle,
      tab cycle, digit jumps, `g s`).
- [x] The help overlay title names the active tab (e.g. `Keybindings — Graph`).
- [x] No tab's help section references a binding that doesn't dispatch on
      that tab (i.e. no Tasks bindings leaking onto Graph).
- [x] `Tab` trait grows a `help_sections(&self) -> Vec<HelpSection>` method
      with a default empty implementation so adding new tabs stays cheap.
- [x] Snapshot tests cover the `?` overlay on each of the four tabs.
- [x] A behaviour test switches tabs, opens `?`, and asserts the rendered
      title reflects the active tab.
- [x] `docs/architecture.md` gains a short note under "Adding things" about
      how to surface help when adding a new tab.

## Technical Notes

### Per-tab keybinding inventory (collected by reading the tabs)

**Global (App-level, all tabs)**
- `q` / `Ctrl+C` quit
- `?` toggle help
- `Tab` / `Shift+Tab` next / previous tab
- `1`–`4` jump to tab N
- `g s` git sync (leader)
- `Esc` close overlay (in modal/help contexts)

**Graph tab (`tabs/graph.rs`)**
- Tree nav: `j`/`k`/`↑`/`↓`, `g`/`G`, `Ctrl+D`/`Ctrl+U`, `h` collapse/parent,
  `Enter` or `l` toggle expand
- File ops: `o` open in editor, `Ctrl+O` open in Obsidian, `c` create blank,
  `C` create from template, `r` refresh
- Move: `m` start (then `m` confirm source, `t` switch to picker, `/` query)
- Periodic leader: `p` then `d`/`w`/`m`/`q`/`y`; also `t` quick-daily
- Multi-view: `Ctrl+N` new view, `Ctrl+W` close, `Ctrl+PageDown`/`Ctrl+PageUp`
  cycle, `Alt+1..9` jump
- Query input mode (after `/`): standard edit-buffer keys

**Notes tab (`tabs/notes/`)**
- Idle: `o` open picker, `m` move flow, `c`/`C` create, `t` daily,
  `p` periodic leader
- Periodic leader: same as Graph
- Pickers/forms: standard FuzzyPicker + edit-buffer keys; `Ctrl+N` in target
  picker to create new target; multi-select `Space`/`j`/`k`/`Enter`; compose
  `r` rename, `K`/`J` reorder, `h`/`l` level

**Tasks tab (`tabs/tasks/`)**
- Normal: `/` query, `j`/`k`/`↑`/`↓` select, `R` reload, `]`/`[` due ±1d,
  `}`/`{` sched ±1d, `t` due=today, `p`/`P` priority cycle, `x`/`X`
  complete/cancel, `e` edit popup, `c` quickline, `C` blank form,
  `Enter` open task in editor
- Query-edit / quickline / edit-popup / target-picker: edit-buffer + picker
  keys; `Ctrl+E` expand quickline to form; `Ctrl+S` submit form;
  `Tab`/`Shift+Tab`/`↑`/`↓` cycle form fields

**Timeblocks tab (`tabs/timeblocks/`)**
- Nav: `j`/`k`/`g`/`G`/`h`/`l` (pane focus / day nav depending on Split/Single)
- Mutations: `r` refresh, `c` create-daily, `a` quickline, `A` form,
  `e` edit-desc, `d d` delete (two-stroke), `]`/`[` end-time ±5m,
  `}`/`{` start-time ±5m, `>`/`<` shift ±5m, `f` toggle split/single,
  `H`/`L` slide day, `T` jump to today, `t` tag modal

### Phase 1 — per-tab help (this plan)

1. New module `ft/src/tui/help.rs`:
   ```rust
   pub struct HelpEntry { pub keys: String, pub desc: String }
   pub struct HelpSection { pub title: String, pub entries: Vec<HelpEntry> }
   pub fn global_section() -> HelpSection { /* quit, ?, Tab, digits, g s, Esc */ }
   ```
   `keys` is a pre-rendered display string ("Ctrl+E", "Shift+C", "g s") so
   Phase 2 can swap in a parsed `KeySpec` later without changing the help
   renderer.

2. Extend `Tab` trait in `ft/src/tui/tab.rs`:
   ```rust
   fn help_sections(&self) -> Vec<HelpSection> { Vec::new() }
   ```
   Default empty so existing tests keep compiling between sessions.

3. Rewrite `render_help_overlay` in `ft/src/tui/ui.rs` to accept
   `tab_title: &str`, `tab_sections: &[HelpSection]`, `global: &HelpSection`.
   Render: title line `Keybindings — <tab>`, then the Global section, then
   each tab section with its own sub-heading. Drop the old `HELP_LINES`.

4. `App::draw` reads `self.tabs[self.active].help_sections()` when
   `Mode::Help` and passes it to the overlay along with the global section.

5. Each tab implements `help_sections()` from local `const` data, grouped
   into sub-sections (Graph: Navigation / Files / Move / Periodic / Views;
   Tasks: Navigation / Mutations / Modals; Notes: Idle / Periodic / Pickers
   & flows; Timeblocks: Navigation / Mutations / Modals).

6. Snapshot tests in `ft/src/tui/snapshots/` for `?` on each tab; one unit
   test that asserts every tab returns at least one non-empty section.

### Phase 2 — Action / KeyMap layer (designed here, NOT built)

Goal: a single source of truth that drives both dispatch and help, and that
a config file can override.

- Introduce an `Action` enum per tab (e.g. `GraphAction::OpenInEditor`,
  `TasksAction::CompleteSelected`). Each variant carries a static
  description used by help rendering.
- `KeySpec` parses strings like "Ctrl+E", "Shift+Tab", "g s" into a
  structured chord (head key + optional follow-up).
- `KeyMap<A>` = `Vec<(KeySpec, A)>` (vector, not hashmap, so leader chords
  resolve deterministically).
- Each tab keeps a `KeyMap<TabAction>` field, built from a default table at
  construction time. `handle_event` becomes a single
  `match keymap.lookup(event_buffer)`.
- `help_sections()` derives entries from the map: for each `Action`, the
  binding list = `KeySpec`s that map to it; sub-section titles come from a
  per-action `category()` accessor.

### Phase 3 — Config override (designed, separate future plan)

```toml
[tui.bindings.global]
quit  = "q"
help  = "?"

[tui.bindings.graph]
open_in_editor   = "o"
open_in_obsidian = "Ctrl+O"
move_section     = "m"
```

Loader: defaults + user overrides → merged `KeyMap` at startup. Conflicting
bindings surface as a warning toast (same surface as plan-014 sync errors).
Live reload deferred until later.

### Out of scope (Phase 1)
- KeyMap / Action refactor (Phase 2)
- Config-driven overrides (Phase 3)
- Searchable / scrollable help overlay
- Context-sensitive sub-state help (e.g. "what does `r` do *inside* the
  compose step of the move flow"). Phase 1 shows the top-level inventory
  for the active tab regardless of sub-state.

### Files touched (Phase 1)
- `ft/src/tui/help.rs` (new)
- `ft/src/tui/mod.rs` — add `pub mod help;`
- `ft/src/tui/tab.rs` — add `help_sections` default method
- `ft/src/tui/ui.rs` — drop `HELP_LINES`; rewrite `render_help_overlay`
- `ft/src/tui/app.rs` — wire active tab's help into the draw path
- `ft/src/tui/tabs/graph.rs` — impl `help_sections`
- `ft/src/tui/tabs/notes/mod.rs` — impl `help_sections`
- `ft/src/tui/tabs/tasks/mod.rs` — impl `help_sections`
- `ft/src/tui/tabs/timeblocks/mod.rs` — impl `help_sections`
- `ft/src/tui/snapshots/` — new help snapshots per tab
- `docs/architecture.md` — short note under "Adding things"

## Sessions

### Session 1 · 2026-05-25 · done
**Goal:** Land the `help.rs` module, the `Tab::help_sections` trait method, the
new overlay renderer, and the four tab implementations behind one PR. Include
per-tab snapshot tests and the architecture-doc note.
**Outcome:**
- Added `ft/src/tui/help.rs` with `HelpEntry`, `HelpSection`, and `global_section()`.
  Key strings are `String` (pre-rendered) so Phase 2 can plug in a `KeySpec`
  parser without churn at the renderer or tab boundary.
- Added `Tab::help_sections(&self) -> Vec<HelpSection>` with empty default;
  implemented on all four tabs:
  - Graph: Navigation / Query / Files / Move / Periodic / Views (6 sections)
  - Tasks: Navigation / Mutations / Create-edit / Text input (4 sections)
  - Notes: Notes / Periodic / In any picker · form / Move flow — compose
  - Timeblocks: Navigation / Edit times / Create-edit-delete / Modals
- Rewrote `ui::render_help_overlay` to take `(tab_title, global, tab_sections)`
  and compose them with shared key-column padding. Bumped popup to 75% × 95%
  to fit the longer Graph/Tasks lists at 80 cols.
- `App::draw` reads the active tab's `help_sections()` when `Mode::Help`.
- Removed the Notes-tab-private `?` interception (`show_help` field +
  `render_help_overlay` in `notes/view.rs`); the Notes tab now flows through
  the standard global help path.
- Tests:
  - `help_overlay_documents_every_canonical_tasks_binding` (renamed; asserts
    each Tasks label appears).
  - `help_overlay_header_names_active_tab` — switches across all 4 tabs and
    asserts `Keybindings — <Tab>` appears in each.
  - `every_tab_returns_non_empty_help_sections` — guards against an empty
    tab block in `?`.
  - Updated snapshots for `help_overlay_80x24`, `help_overlay_over_tasks_80x24`,
    `notes_help_overlay_80x24`; added `timeblocks_help_overlay_80x24`.
- `docs/architecture.md` gained an "A new TUI tab" subsection under
  "Adding things" pointing at `help_sections`.
- Test counts: `ft` 316 → 319 passing; 36 unchanged pre-existing fixture
  failures (graph_*) — verified by `git stash` against `main`. `cargo fmt`
  and `cargo clippy --workspace --tests -- -D warnings` clean.
