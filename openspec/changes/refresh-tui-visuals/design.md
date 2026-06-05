## Context

The TUI currently uses a cool-toned palette (cyan highlights, magenta accents, yellow key-binding labels) against a dark background. The graph tab's tree viewport has no border frame, the query bar sits at the bottom, and the timeblocks tab defaults to a side-by-side two-pane split.

All TUI rendering lives in `ft/src/tui/` with ratatui as the UI library. Color values are inlined as `Color::Cyan`, `Color::Yellow`, etc. across render functions — there is no centralized theme module. The graph tab's `render` body is a vertical split of [strip, tree, input_bar], with the input bar at the bottom. Timeblocks uses `ViewMode::Split` as its `Default` initial value.

## Goals / Non-Goals

**Goals:**
- Define a warm color palette (orange/red/yellow tones) and apply it consistently to all TUI elements
- Add a bordered frame around the graph tab's tree viewport
- Move the graph tab's query input bar from the bottom to the top of the body area
- Change the timeblocks tab's initial view mode to Single-day, keeping `f` as the toggle

**Non-Goals:**
- Theming engine or user-configurable colors (v2 feature)
- Changing the notes tab's layout (it already has a border and no search bar)
- Changing keybindings or command names
- Changing any ft-core code

## Decisions

### 1. Color palette: orange/red/yellow warm tones

Centralize color constants in a new module `ft/src/tui/palette.rs` with named constants for each semantic role (primary, secondary, accent, dim, bg, success, error, etc.). This avoids scattering literal `Color::Rgb(r, g, b)` values across render functions. Every render site that currently uses `Color::Cyan` / `Color::Magenta` gets replaced with the corresponding palette constant.

**Palette mapping:**

| Semantic role | Current color | New color | Usage |
|---|---|---|---|
| Primary accent (highlights, active borders) | Cyan | Orange `Color::Rgb(255, 165, 0)` | Tab bar highlight, focused pane border, active query bar |
| Secondary accent (key labels, chords) | Yellow | Gold/yellow `Color::Rgb(255, 200, 50)` | Keybinding labels in help, status bar hints |
| Tertiary accent (modals, in-flight indicator) | Magenta | Warm red `Color::Rgb(255, 80, 80)` | Modal names, modal hints |
| Success | Green | Keep green (contrast with red) | Success toasts |
| Error | Red | Keep red | Error toasts, error text |
| Dim (inactive, separators, hints) | DarkGray | Warm gray `Color::Rgb(100, 85, 70)` | Status bar labels, dividers, hints |
| Info toasts | Cyan | Orange (same as primary) | Info toasts |
| Background (modals, panels) | Black | Keep black | Modal backgrounds |
| Status bar bg | `Color::Rgb(28, 28, 32)` | Slightly warmer `Color::Rgb(30, 28, 26)` | Status bar background |
| Help overlay bg | Black | Keep black | Help overlay |
| Selected row bg | Black/White inverse | Orange bg `Color::Rgb(255, 165, 0)` with white fg | Selected rows in lists |

**Alternatives considered:**
- Defining colors via `Style` constants: too rigid — many sites build `Style` on-the-fly with modifiers. Named colors are more flexible.
- User-configurable palette: out of scope for this change. The module structure supports future config-read.
- Using a more subtle orange (darker): tried `Color::Rgb(200, 130, 30)` — feels muddy on terminals. Bright orange pops.

### 2. Graph tab tree frame

Add a `Block::default().borders(Borders::ALL).title(...)` around the tree area in `GraphTab::render`. The title shows the active view's query snippet (same as the view-tab strip). The border color uses the primary accent (orange). The frame matches how the timeblocks panes and notes idle panel use `Block` with `Borders::ALL`.

The tree area constraint remains `Constraint::Min(1)` — no height change needed.

**Alternative considered**: Wrapping the entire body (strip + tree + input) in a frame — rejected because the strip and input bar already act as chrome, and a double-frame looks noisy.

### 3. Graph query bar at top

Current layout in `GraphTab::render`: `[strip(1), tree(Min), input(1)]`  
New layout: `[input(1), strip(1), tree(Min)]`

The view-tab strip (`1: query... 2: query...`) moves below the query bar. The strip remains 1 row. The input bar height remains 1 row. No other structural changes needed. The `input_mode` flag and cursor positioning logic are unchanged — they still operate on `input_area` which is now `chunks[0]` instead of `chunks[2]`.

When in query-bar editing mode (`/`), the `>` prompt appears at top with cursor. When not editing, it shows the current query text dimmed, exactly as today — just at the top.

### 4. Timeblocks single-day default

Change `ViewMode::Split` to `ViewMode::Single` in `TimeblocksTab::new` and `TimeblocksTab::with_clock`. The `ViewMode::Split` variant remains fully functional — `f` still toggles between modes. The sidebar label updates to reflect the current mode (already does: "view: single (f)" / "view: split"). No render changes in `view.rs` needed — it already branches on `tab.view`.

## Risks / Trade-offs

- [Color visibility on light terminals] → TUI already assumes dark background. The warm palette is designed for dark backgrounds; light-terminal users already have a suboptimal experience.
- [Orange-on-white selected row could be low contrast on some terminal emulators] → Mitigated by using bold modifier and testing on common emulators (kitty, alacritty, iTerm2, Windows Terminal via snapshot tests).
- [Graph query bar at top changes muscle memory for existing users] → Low risk: the tasks tab already has query bar at top. The `/` keybinding still opens the query bar in-place.
- [Single-day default might surprise power users who rely on the split] → The `f` key is visible in both the help overlay and the sidebar, and the default change is persistent across sessions since there's no session persistence for view state today.

## Open Questions

- Should the tab bar highlight color be the same orange as primary accent, or a slightly different shade? → Use the same orange for consistency.
- Should the timeblocks sidebar border color change to match the new palette? → Yes, sidebar border should use the primary accent (orange) instead of DarkGray.
