## Context

Three TUI feed tabs render paragraph/link lists. Today:

- **Pulse** (`ft/src/tui/tabs/pulse.rs`) renders its `[[link]]` rows into
  one `Paragraph` with no viewport-follow — the cursor can move below the
  fold but the view never scrolls, so links past the first screen are
  unreachable. The codebase already has a shared
  `ft/src/tui/widgets/scroll_list.rs::render_scroll_list` (ratatui `List`
  + `ListState` + right-edge `Scrollbar`) used by every other scrollable
  list; Pulse just doesn't use it.
- **Recent** (`ft/src/tui/tabs/recent.rs::render_history`) and **Gather**
  (`ft/src/tui/tabs/gather.rs::render_gather`) render every entry as a
  tall block: a full-width header band + up to two badge sub-lines
  (`matched:` for Gather multi-source; `cited:`/`cited*:`/`in note`/
  `missing`) + the full wrapped paragraph body + a blank separator. With
  more than a few entries this is a wall of prose where entries are hard
  to distinguish — the problem an email-client split (compact list on
  top, preview on the bottom) solves.

`RecentEntry` and `GatherEntry` are near-identical (`GatherEntry =
RecentEntry + matched: Vec<NoteId>`), and both tabs already share the
`inline_markdown_spans` / `wrap_line` / `pad_to_width` / `citation_badge_line`
helpers in `gather.rs`. So the split layout can be one shared widget that
takes per-entry row/header/body builders.

Constraints (from AGENTS.md):
- TUI is single-threaded except two producer threads; no new threading.
- Tabs read the shared `Arc<GraphSnapshot>` via `ctx.snapshot`; they do
  not `vault.scan()`/`Graph::build`. This change is render-only — no new
  data fetching.
- Command/keymap registry is the single source of truth; `?` overlay,
  `docs/keybindings.md`, `ft commands list` read it. We add **no**
  commands and change **no** keymap chords, so the registry and
  `docs/keybindings.md` stay in sync (regenerated only if a help-section
  *description* changes).
- Insta snapshots for stable output formats must be re-recorded when the
  output changes.

## Goals / Non-Goals

**Goals:**
- Pulse viewport auto-follows the cursor + scrollbar on overflow,
  matching every other scrollable list.
- Recent and Gather move to a list/preview split with a stable-height
  (10-row) list pane and a non-scrolling preview pane.
- One shared `feed_split` widget renders both paragraph tabs; per-tab
  badge construction stays local.
- Multi-select, send-to-synth, move-section, open-in-editor, reload, and
  all filters behave identically to today.
- All five build invariants stay clean; affected snapshots re-recorded.

**Non-Goals:**
- Independent preview-pane scroll (Q3 answer: no — long paragraphs are
  visibly cut; `Enter` opens in `$EDITOR`).
- Converting Pulse to the split layout (Pulse has no paragraph body to
  preview; it only gains scroll-follow).
- Changing any command, keymap chord, CLI, or data format.
- Touching the synth scaffold/callout, move-modal, or handoff flows.

## Decisions

### D1 — Pulse uses `render_scroll_list` directly
**Decision.** Replace Pulse's hand-rolled `Paragraph::new(lines)` render
with `render_scroll_list`, passing `ListItem`s built from the same
`(count) [[target]]?` rows and `selected = Some(self.cursor)`.

**Why.** `render_scroll_list` already encapsulates the
`List`+`ListState`+`Scrollbar` pattern and is the codebase's stated
single home for that. It auto-scrolls the selection into view (ratatui
recomputes the offset from `selected` each frame) and draws the
right-edge scrollbar on overflow. Multi-select markers (`[*]` today in
Pulse) become a `ListItem` prefix, consistent with how the gather/recent
tabs render `●`.

**Alternatives.** Hand-roll `Paragraph::scroll((offset,0))` with manual
offset math (what recent/gather do today). Rejected: duplicates
`render_scroll_list` and drifts from the established look.

### D2 — One shared `feed_split` widget for the two paragraph tabs
**Decision.** New `ft/src/tui/widgets/feed_split.rs` exposing:

```text
render_feed_split(
    frame, area,
    list_rows: Vec<ListItem>,          // one line per entry, already styled
    selected: usize,
    multi_selected: &HashSet<usize>,
    preview_header: &[Line],          // distinct header lines (title/date/range/badges)
    preview_body: &[Line],            // wrapped paragraph body
)
```

It splits `area` vertically into `[list (min(height, LIST_DEFAULT=10, entries.len()))][preview]`,
renders the list via `render_scroll_list` (so cursor-follow + scrollbar
come for free), draws a separating rule under the preview header, and
renders the header+body into the preview pane. Empty/loading/error states
are handled by the *caller* before calling the widget (full-pane message,
no split drawn) — the widget assumes a non-empty feed.

**Why.** Both tabs compute the same things per entry (a compact row, a
header, a wrapped body). Factoring the split *geometry* + list rendering
into one place keeps the two tabs' layouts guaranteed-identical and the
badge logic local. `RecentEntry`/`GatherEntry` differences (Gather's
`matched` field, Gather's Sources strip) stay in the tab.

**Alternatives.**
- *Inline the split in each tab.* Rejected: two copies of the geometry
  math + list/scrollbar wiring; the user explicitly asked for reuse.
- *A trait object that yields rows/headers/bodies.* Rejected: overkill —
  the two tabs differ only in which fields they read; passing built
  `Vec<ListItem>`/`Vec<Line>` is simpler and allocation-cheap for the
  feed sizes involved.

### D3 — List pane height: stable 10, clamped to entry count
**Decision.** `list_height = min(LIST_DEFAULT, entries.len()).min(area.height)`,
`LIST_DEFAULT = 10`. The preview pane takes `area.height - list_height`.
Heights are recomputed only from `entries.len()` and `area`, **not** from
the cursor, so they stay stable while browsing.

**Why (Q1 answer).** A stable list height keeps the preview pane size
constant as the user arrows around — the property the user asked for.
Clamping to `entries.len()` avoids a half-empty list pane for small
feeds.

**Alternatives.** Proportional split (e.g. 40/60). Rejected: preview
height would vary with terminal size in a less predictable way, and a
fixed row count is the more common email-client default.

### D4 — Compact list row: `{date} {title}` + inline citation badge
**Decision (Q2 answer = option b).** Each list row is one
`ListItem`:
`{date} {source_title} {citation_badge?}`, where the citation badge is
the compact form (`cited: Syn`, `cited*: Syn`, `in note`, `missing`) and
is omitted entirely when there's nothing to show (uncited in global
mode). Multi-select prefix `●`; cursor highlight via `render_scroll_list`'s
`highlight_style`. Rows do not wrap; `pad_to_width`-truncate to the list
pane width (reuse the existing helper).

The `matched:` badge (Gather multi-source) is **not** on the list row —
it goes in the preview header (Q4).

**Why.** Citation state is the signal the user scans for at a glance (it
drives `u`/`o`/send-to-synth). `matched:` is noisier and per-entry
rarely varies, so it fits the preview header better.

**Alternatives.** (a) badges only in preview header; (c) both badges
inline. Rejected per the user's Q2 = (b).

### D5 — Preview header: distinct colors + separating rule, full detail
**Decision (Q4 answer).** Preview pane = header block + body. Header
uses a distinct color (e.g. `palette::TERTIARY` fg on a
`palette::ENTRY_HEADER_BG` band, BOLD) and a single-line rule
(`─` repeated to pane width, `palette::DIM`) separates it from the body.
Header lines:
1. `{source_title}  ·  {date}  ·  L{line_start}–{line_end}`
2. (Gather, multi-source, `matched.len() > 1` only) `matched: Foo, Bar`
3. citation detail:
   - global + cited: `cited: Syn, Other`
   - global + stale: `cited*: Syn` (staleness surfaced distinctly)
   - context-note mode: `in note` / `missing` (the badge the list row
     already shows, repeated here in full)
   - uncited (global): no citation line

`citation_badge_line` (in `gather.rs`, already `pub(crate)`) is reused
for the badge value; the *detail* (list of citing note stems) is derived
from the `CitationState` directly in the tab (it already has
`state.cited_in(note)` / `notes` fields).

**Why.** The header must be self-contained (the user reads the preview
without looking at the list) and must surface staleness, which the
compact list badge abbreviates.

### D6 — Preview body is non-scrolling; long paragraphs cut off
**Decision (Q3 answer).** Preview body = the selected entry's
`section_text`, wrapped via the existing `wrap_line` to the preview pane
width, rendered as `Paragraph` (no `.scroll()`). When the wrapped body
exceeds the body height, the extra lines simply aren't drawn — ratatui
clips. No scrollbar, no preview-scroll keys. Moving the cursor re-renders
the preview from the top for the new entry (no offset state to reset —
the preview is stateless per render).

**Why.** Simplest possible; no new keymap row; matches "I just want to
read the current paragraph, and I can `Enter` to open it in full." A
future change can add independent preview scroll if the cut-off proves
annoying.

**Alternatives.** Independent preview scroll with `PgUp`/`PgDn`.
Rejected per the user's Q3.

### D7 — Gather's Sources strip stays above the split
**Decision.** Gather's layout becomes
`Sources strip (2 rows) / list pane / preview pane`. `render_sources_strip`
and its 2-row reservation are unchanged; the `feed_split` widget is given
the `body_area` that's already below the strip. Recent has no strip, so
its split gets the whole inner area.

**Why.** The Sources strip is the Gather tab's load-state signal (loaded
sources / window / filters / context note) and is independent of the
per-entry layout. Keeping it above the split preserves that signal and
avoids reflowing it into the preview header.

### D8 — Multi-select + synth flows unchanged
**Decision (Q5 answer).** `Space` toggles `entry_selected` on the cursor
row exactly as today; the list row shows `●` for selected rows;
send-to-synth / move-section / open-in-editor / reload / filters all
operate on the same `entry_selected` set and `selected` cursor. The
preview always shows the cursor row (not a separate "preview selection").

**Why.** The user confirmed multi-select semantics are unchanged. The
split is purely a rendering change; the tab's *state* (`entries`,
`selected`, `entry_selected`) is untouched.

### D9 — Snapshot re-records; no contract regression
**Decision.** The existing insta snapshots that capture these tabs'
rendered output will change and must be re-recorded:
`journal_entry_blocks_80x24.snap`, the
`journal_tab_selected_entry_body_always_visible` /
`journal_tab_entry_blocks_layout` tests, `history_tab_renders_recent_feed`
assertions, and any Pulse snapshot. The *intent* of
`journal_tab_selected_entry_body_always_visible` (the selected entry's
body is visible) is preserved by the preview pane showing the cursor
row's body — the test stays, its snapshot updates. New snapshots cover the
split layout (selected entry's header + cut-off body) and Pulse
scroll-follow. No command/keymap *chord* changes, so
`ft commands docs --check` stays green (regenerate
`docs/keybindings.md` only if a help-section *description* is reworded).

## Risks / Trade-offs

- **[Preview cut-off hides context]** → Mitigation: the preview header
  always shows line range + citation detail, and `Enter` opens the full
  paragraph in `$EDITOR`. A follow-up can add independent preview scroll.
- **[List-row truncation hides the title on narrow terminals]** →
  Mitigation: the preview header repeats the full title; the list row is
  a scan aid, not the sole source of truth.
- **[Snapshot churn masks a real regression]** → Mitigation: keep the
  *behavioral* tests (`journal_tab_selected_entry_body_always_visible`
  intent, multi-select → synth count, handoff) as assertion-based, and
  only re-record the *visual* snapshots; review diffs before accepting.
- **[Pulse look changes when switching to `render_scroll_list`]** →
  Mitigation: the change is to the established codebase-wide list look;
  re-record the Pulse snapshot and confirm the cursor is reachable past
  the fold in a new test.
