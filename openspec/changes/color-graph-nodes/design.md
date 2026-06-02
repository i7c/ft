## Context

The graph tab tree renders rows as a single flat `Span::styled(line, style)` where `style` is uniformly white (or black-on-white for the selected row). The row format string is:

```
{indent}{indicator} {sel_marker} {kind_char} {display}
```

Five node types can appear interleaved in the same tree. Users currently distinguish them only by the single-character prefix (`N`, `D`, `G`, `T`, `P`). Color-coding each type makes the tree scannable and reduces visual clutter in dense views.

The coloring is purely a TUI presentation concern — it stays entirely in `ft/src/tui/tabs/graph.rs`.

## Goals / Non-Goals

**Goals:**
- Each `NodeKind` variant gets a distinct, readable foreground color in the tree view.
- The kind character and display text of each row are colored.
- Selected rows preserve the highlight (white background, black foreground for non-type elements) while still showing the type color.
- Ghost and Paragraph node types get visually de-emphasized tones.

**Non-Goals:**
- Coloring the fuzzy picker (`/` search-in-tree) entries — that uses a separate `PickerItem` rendering path and has its own complexity (match highlights). This can be a follow-up.
- Coloring the tab-strip view labels or input bar.
- User-configurable colors (this can come later via `[graph.colors]` in config).
- Changing the terminal theme or background color — we use `ratatui::style::Color` named and ANSI colors only.

## Decisions

### 1. Color mapping: static function in the TUI layer

A free function `fn node_kind_color(kind: &NodeKind) -> Color` lives in `ft/src/tui/tabs/graph.rs` next to `leaf_display`. It maps each `NodeKind` variant to a fixed `Color`. This is separate from `leaf_display` because display text and presentation color are separate concerns.

**Alternatives considered:**
- Embedding color in `leaf_display`'s return value → couples content and presentation at the wrong layer.
- Reading colors from `Config` → over-engineered for v1; can add `[graph.colors]` later if users ask.

### 2. Per-node-type color palette

| Node kind | Color | Rationale |
|-----------|-------|-----------|
| Note | `Cyan` | Cool, calm — the default "file" color in many tools |
| Directory | `Blue` | Traditional folder color (Finder, Nautilus, `ls --color`) |
| Ghost | `DarkGray` | Dimmed — these are "not real files"; shouldn't compete for attention |
| Task | `Yellow` | Action items — warm, attention-grabbing |
| Paragraph | `Gray` | Muted, distinct from cyan Note — inline sub-content |

These colors work on both dark and light terminal backgrounds. The selected-row highlight (white background) contrasts adequately with all of them, and the type colors remain distinguishable even against white.

### 3. Row rendering: `Line` from multiple `Span`s

Currently each row is a single `Span::styled(line_string, uniform_style)`. We switch to building a `Line` from multiple `Span`s:

```text
Span("  ▶ ● ",  base_style)    // indent + indicator + sel_marker + " "
Span("N",       kind_style)    // kind_char in type color
Span(" ",       base_style)
Span("My Note", kind_style)    // display in type color
```

Where:
- `base_style` = `Style::default().fg(Color::White)` (or `.fg(Color::Black).bg(Color::White)` when selected)
- `kind_style` = `base_style.fg(kind_color)` — inherits the bg modifier from base for selected rows

This gives: type-colored text on the normal background (unselected) or type-colored text on white background (selected).

### 4. No changes to `leaf_display` or `core`

`leaf_display` continues returning `(String, char)`. The color function is separate and invoked in the render method. No `ft-core` changes at all — this is purely a presentation concern.

## Risks / Trade-offs

- **Color-blind accessibility**: Cyan/Blue/Yellow/Gray/DarkGray palette has luminance and hue differences. However, the kind character prefix is preserved as a redundant cue. No information is conveyed by color alone. → Acceptable.
- **Terminal color support**: We use standard ANSI named colors (Cyan, Blue, Yellow, Gray, DarkGray). All modern terminals support these. No true-color or 256-color needed. → Low risk.
- **Snapshot churn**: Every TUI test snapshot that renders the graph tab will need regeneration. This is mechanical — re-run `cargo test` with `INSTA_UPDATE=always` after the change. → Acceptable, documented in tasks.
