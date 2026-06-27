## Context

The `?` keymap-help overlay is the TUI's primary discovery surface — it
shows the chords bound in the active context (tab or modal) plus the
App-global bindings. Today it is rendered by
`ft::tui::ui::render_help_overlay`, which walks the global `KeyMap`
section and the active tab/modal sections (built by
`ft::tui::help::sections_from_keymap`), concatenates every row into a
single `Vec<Line>`, and renders `Paragraph::new(lines).block(block)`
into a `centered_rect(75, 95, area)` popup — no scroll state, no
scrollbar.

The heaviest tab (Graph) has ~35 bindings across several groups plus
the ~6-row global section, which overflows anything under ~45 lines.
The 95%-height popup is the documented workaround ("sections render
top-to-bottom without scrolling so the popup needs the room"), but it
still overflows on short terminals and collides with the status bar.
While `mode == Help`, `App::handle_event` swallows every key except
`Esc`/`?`/`q`, so there is no way to scroll even if scrolling existed.

Established patterns in the codebase:

- `tabs/journal.rs:1379` renders a read-only scrolled pane via
  `Paragraph::new(lines).scroll((*scroll_offset as u16, 0))`.
- `widgets/scroll_list.rs::render_scroll_list` draws a `Scrollbar` +
  `ScrollbarState` on the right edge when content overflows the area,
  reserving the rightmost column so the thumb never paints over text.
  Both call sites pass a caller-owned `usize` offset / `selected`
  index.

`Mode::Help` is an App-level `Mode` (alongside `GitLeader`,
`SyncConflict`), not an `ActiveModal` variant. The Modal trait's
`handle_event` / `keymap_help` machinery is for multi-step flows with
their own command set; Help is a read-only overlay that displays the
*active context's* keymap, so converting it to a modal would add cost
without benefit.

## Goals / Non-Goals

**Goals:**

- The `?` overlay shows *all* of the active context's bindings on any
  terminal height, by scrolling when content overflows the popup.
- Scroll interaction is discoverable: a scrollbar appears on overflow
  (matching every other scrollable surface in the TUI), and the footer
  hint names the scroll keys.
- Scroll keys are consistent with editor/tmux conventions already used
  in the app (`j`/`k`, `PageUp`/`PageDown`, `g`/`G`).
- Zero churn to the `CommandDef`/`KeyMap` registry — scroll keys are
  mode-local, so `ft commands docs --check` and `docs/keybindings.md`
  are unaffected.

**Non-Goals:**

- Horizontal scrolling or text wrapping changes. Long descriptions
  truncate at the popup edge as today.
- Re-flowing which rows render (the keymap/registry walk in
  `sections_from_keymap` is untouched).
- Converting `Mode::Help` into an `ActiveModal` variant.
- Filter/search-within-help (typing to jump to a binding). Out of
  scope; the scroll fix is the reported problem.
- Persisting scroll position across open/close. Each open starts at
  the top, which is what users expect for a help overlay.

## Decisions

### D1: Keep `Mode::Help` an App-level Mode; add scroll keys inline in `handle_event`

The scroll keys are handled in the existing `if self.mode == Mode::Help`
branch of `App::handle_event`, the same way `GitLeader` handles `s` and
`SyncConflict` handles dismiss. These mode-local keys are **not**
`CommandDef`s: they exist only inside one `Mode`, have no stable
`<context>.<verb>` name, and are not part of the registry that
`ft commands list` / `docs/keybindings.md` consume. Registering them
would force a fake context and pollute the global command set.

**Alternative considered:** make the help overlay an `ActiveModal`
variant and use `Modal::dispatch_command`. Rejected because the Modal
trait is for multi-step flows with their own keymap; Help displays the
*active context's* keymap, so it would need to proxy to a varying
keymap, and the trait's `ModalOutcome`/`keymap_help` machinery would
be dead weight. AGENTS.md's "prefer an `ActiveModal` variant" guidance
targets per-tab modal *state*, not App-global read-only overlays.

### D2: Persist scroll as `Cell<usize>` on `App`, plus a `Cell<u16>` view height

`App::draw(&self)` takes `&self`, so mutable state lives in interior-
mutability cells — this is exactly why `last_refresh`, `toast`, and
`sync_conflict` are `Cell`/`RefCell`. Two cells:

- `help_scroll: Cell<usize>` — the line offset, clamped each render.
- `help_view_height: Cell<u16>` — the last-rendered inner popup height,
  written by `render_help_overlay`. The page-step keys (`PageDown`,
  `PageUp`) need a real viewport size to scroll one page; reading it
  back from the cell avoids guessing with a magic constant.

The renderer computes `max_scroll = lines.len().saturating_sub(inner
height)` and writes the clamped offset back through a `&mut usize`
parameter, so `help_scroll` can never drift past the valid range even
if the terminal is resized between key press and render.

**Alternative considered:** step `PageDown` by a fixed constant (e.g.
8) and rely on the per-render clamp. Rejected — imprecise page steps
feel broken on tall terminals, and stashing the real height is cheap
(one cell write per frame).

**Alternative considered:** compute scroll purely in the renderer
(auto-follow) with no key handling. Rejected — the user needs to be
able to look at *arbitrary* rows, not just follow a selection, and the
report explicitly asks for scrolling.

### D3: Render via `Paragraph::scroll` + a right-edge `Scrollbar`, mirroring `scroll_list`

Inside the popup, after the `Block` is drawn:

1. `inner = block.inner(popup)`.
2. `max_scroll = lines.len().saturating_sub(inner.height as usize)`;
   `*scroll = (*scroll).min(max_scroll)`.
3. If `lines.len() > inner.height`: reserve the rightmost column for the
   scrollbar track (`text_area.width = inner.width - 1`), render
   `Paragraph::new(lines).scroll((*scroll as u16, 0))` into `text_area`,
   then render `Scrollbar::new(ScrollbarOrientation::VerticalRight)` +
   `ScrollbarState::new(lines.len()).viewport_content_length(inner
   .height).position(*scroll)` into the rightmost column, copying the
   thumb/track styling from `scroll_list.rs` so the two scrollbars are
   visually identical.
4. Else: render the paragraph into the full `inner` with no scrollbar.

**Alternative considered:** replace the `Paragraph` with the
`widgets::scroll_list::render_scroll_list` helper. Rejected — that
helper renders a `List` of `ListItem`s with a highlight symbol and
selection semantics, but the help overlay renders pre-styled multi-
`Span` `Line`s (key column bold/secondary, desc column white) with no
selection. Reusing it would mean either losing the per-span styling or
adding a no-selection passthrough mode that duplicates the paragraph
path. Keeping `Paragraph::scroll` + a hand-rolled scrollbar (≈15 lines,
copied from `scroll_list`) preserves the rich styling and is the
smaller change.

### D4: Scroll key set

| Key | Action |
|-----|--------|
| `j` / `↓` | line down (+1) |
| `k` / `↑` | line up (−1) |
| `PageDown` / `Space` | page down (+view_height) |
| `PageUp` / `b` | page up (−view_height) |
| `g` | home (0) |
| `G` | end (max_scroll) |
| `Esc` / `?` / `q` | dismiss (unchanged) |

`Space` aliases `PageDown` and `b` aliases `PageUp` to match
less/man conventions and the app's existing vim-ish leanings. `g`/`G`
are cheap and commonly expected. All are mode-local. The footer hint
reads, e.g.:

```
↑/↓ or j/k scroll · PgUp/PgDn · g/G · ?/Esc/q close
```

### D5: Reset scroll on entry

`help_scroll.set(0)` in three places: the `"app.help"` command arm in
`App::dispatch_global` (or wherever `self.mode = Mode::Help` is set),
the test helper `App::enter_help`, and anywhere else that sets
`mode = Mode::Help`. A grep for `self.mode = Mode::Help` covers the
entry points; each must reset. This guarantees a fresh open always
shows the top, which matches user expectation for a help surface.

## Risks / Trade-offs

- **[Scroll keys shadow tab bindings that use the same letters]**
  → Mitigation: the keys are active *only* while `mode == Help`, and
  Help already swallows all keys except its dismiss set. No tab or
  modal keymap is consulted while Help is open, so there is no actual
  collision — `j`/`k`/`g`/`G`/`b`/`Space` are simply new within-Help
  verbs. The only behavioural change for existing users is that these
  keys (previously swallowed) now scroll instead of no-op.

- **[Footer hint byte-shift breaks existing snapshots]**
  → Mitigation: expected and acceptable. `insta review` the
  `help_overlay_*` snapshots; only the footer line changes. New
  scroll-behavior snapshots are added separately so the existing ones
  stay a focused regression for content/layout.

- **[View-height cell can go stale if the terminal is resized between
  the last render and a page-step key press]**
  → Mitigation: the renderer recomputes and writes `help_view_height`
  every frame, and `draw` runs before any user key is processed in the
  next loop iteration, so the cell reflects the current layout at the
  moment of the key press. Even if it were one frame stale, the
  per-render clamp on `help_scroll` bounds the offset to a valid range,
  so the worst case is a slightly off page step, never an OOB scroll or
  a panic.

- **[Modal-active case shows the modal's keymap, which may also be long]**
  → Mitigation: the same `render_help_overlay` path serves both tab and
  modal help (`app.rs` Mode::Help arm already branches on
  `active_modal`). The fix covers both for free; no separate work.

## Migration Plan

Purely additive UI behaviour; no data, config, or API migration. No
feature flag. Rollout is the implementation PR.

Rollback: revert the PR. The pre-change behaviour (clipped overlay,
swallowed keys) returns with no state to clean up — `help_scroll`/
`help_view_height` are transient frame-local cells with no persistence.
