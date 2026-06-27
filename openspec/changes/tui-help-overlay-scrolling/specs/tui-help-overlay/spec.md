## ADDED Requirements

### Requirement: The `?` help overlay scrolls vertically when content overflows the popup

When the rendered help lines exceed the popup's inner height, the overlay SHALL be scrollable vertically so that every row is reachable. The scroll offset SHALL be a line offset into the composed `Vec<Line>` and SHALL be clamped on every render to `0..=max_scroll`, where `max_scroll = lines.len().saturating_sub(inner_height)`. When the content fits within the popup, no scrolling SHALL occur and no scroll affordance SHALL be rendered.

#### Scenario: Content taller than popup scrolls instead of clipping

- **WHEN** the user opens `?` on a tab whose help lines exceed the popup's inner height (e.g. the Graph tab on an 80×24 terminal)
- **THEN** the overlay renders the top of the content, the bottom rows are not silently clipped, and a scrollbar is visible on the popup's right edge

#### Scenario: Content shorter than popup shows no scrollbar and no scroll

- **WHEN** the user opens `?` on a tab whose help lines fit within the popup's inner height
- **THEN** no scrollbar is rendered and the scroll offset remains `0` under any scroll-key input

#### Scenario: Resize clamps the scroll offset back into range

- **WHEN** the overlay is scrolled down and the terminal is resized shorter so the current offset exceeds the new `max_scroll`
- **THEN** the next render clamps the offset to the new `max_scroll` and no out-of-range offset is retained

### Requirement: Scroll is driven by mode-local keys active only while the help overlay is open

While `mode == Help`, the overlay SHALL respond to a fixed set of mode-local scroll keys: `j` and `↓` scroll down one line, `k` and `↑` scroll up one line, `PageDown` and `Space` scroll down by the popup's inner height (one page), `PageUp` and `b` scroll up by one page, `g` scrolls to the top, and `G` scrolls to the bottom (`max_scroll`). These keys SHALL NOT be `CommandDef`s in the central `CommandRegistry` and SHALL NOT appear in `ft commands list` or `docs/keybindings.md`; they SHALL be handled inline in the `Mode::Help` branch of the App's event handler, matching the precedent of the `GitLeader` and `SyncConflict` modes.

#### Scenario: Line scroll keys move the offset by one

- **WHEN** the help overlay is open and the user presses `j`
- **THEN** the scroll offset increases by 1 (clamped to `max_scroll`) and the visible window moves down one line

#### Scenario: Page scroll keys move the offset by one viewport

- **WHEN** the help overlay is open, the popup's inner height is `H`, and the user presses `PageDown`
- **THEN** the scroll offset increases by `H` (clamped to `max_scroll`)

#### Scenario: Home and end keys jump to the bounds

- **WHEN** the help overlay is open and the user presses `G`
- **THEN** the scroll offset is set to `max_scroll` and the last line of content is visible at the bottom of the popup

#### Scenario: Scroll keys are not registered commands

- **WHEN** `ft commands list` is invoked
- **THEN** no command name corresponds to a help-scroll action, and `docs/keybindings.md` is unaffected by the existence of the scroll keys

#### Scenario: Dismiss keys still close the overlay

- **WHEN** the help overlay is open and the user presses `Esc`, `?`, or `q`
- **THEN** the overlay closes (mode returns to `Normal`) and scroll state is reset on the next open; the scroll keys never preempt dismissal

### Requirement: Scroll position resets to the top on every open

Opening the help overlay SHALL set the scroll offset to `0`, regardless of where it was when the overlay was last closed. This applies to every entry point that sets `mode = Help`, including the `app.help` global command and any test entry helpers.

#### Scenario: Reopening starts at the top

- **WHEN** the user opens `?`, scrolls down, closes the overlay, and opens `?` again
- **THEN** the reopened overlay shows the top of the content (offset `0`)

### Requirement: A scrollbar renders on the popup's right edge on overflow

When the help content overflows the popup, a vertical `Scrollbar` SHALL be rendered in the rightmost column of the popup's inner area, with a `ScrollbarState` whose content length is the total line count, viewport length is the inner height, and position is the current scroll offset. The scrollbar's thumb and track styling SHALL match the `widgets::scroll_list` scrollbar so all scrollable surfaces in the TUI are visually consistent. The rightmost column SHALL be reserved for the track so the thumb never paints over row text.

#### Scenario: Scrollbar reflects position and total

- **WHEN** the help overlay is open and overflowing, the total line count is `T`, the inner height is `H`, and the scroll offset is `O`
- **THEN** a scrollbar is rendered with thumb position proportional to `O` over a track representing `T` lines with a viewport of `H`

#### Scenario: Scrollbar disappears when content fits

- **WHEN** the help overlay's content fits within the popup's inner height
- **THEN** no scrollbar is rendered and the full inner width is available for text

### Requirement: The footer hint documents the scroll keys

The overlay's footer SHALL name the scroll keys alongside the dismiss keys so the interaction is discoverable without external documentation. The hint SHALL mention line scroll (`j`/`k` or arrow keys), page scroll (`PgUp`/`PgDn`), and dismiss (`?`/`Esc`/`q`).

#### Scenario: Footer lists scroll and dismiss keys

- **WHEN** the help overlay is rendered
- **THEN** the footer line contains text referencing scroll keys and the dismiss keys (e.g. `↑/↓ or j/k scroll · PgUp/PgDn · ?/Esc/q close`)

### Requirement: The overlay covers both tab and modal keymap help

The scrolling behaviour SHALL apply uniformly whether the overlay is showing the active tab's keymap or the active modal's keymap, because both are rendered through the same `render_help_overlay` entry point.

#### Scenario: Modal keymap help scrolls when long

- **WHEN** a modal with many bindings is active and the user opens `?`
- **THEN** the modal's help rows scroll using the same keys and scrollbar as tab help
