## Context

The `leaf_display` function in `ft/src/tui/tabs/graph.rs` produces a `(String, char)` pair for each graph node: the display text shown in the tree, and a single-character kind prefix. Currently:

- **Paragraph**: `format!("{}:{}", p.source_file.display(), p.line_start)` — only shows the file and start line. The `text` field (full paragraph body) and `line_end` are available on `ParagraphData` but unused.
- **Task**: `t.description.clone()` — only the description. The `status` field (e.g., `"Open"`, `"Done"`) is available on `TaskData` but unused.

Both node types have rich data already loaded in the graph; the display just isn't using it.

## Goals / Non-Goals

**Goals:**
- Paragraph rows show line range (start-end) and a snippet of the paragraph text.
- Task rows show a compact status indicator alongside the description.
- Changes confined to `leaf_display` in the TUI crate.

**Non-Goals:**
- Changing the storage format or `ParagraphData`/`TaskData` structs in `ft-core`.
- Adding new fields to the graph node types.
- Customizing the truncation length per node or per view.

## Decisions

### 1. Paragraph format: `file:start-end  text_snippet`

Format string: `"{source}:{line_start}-{line_end}  {truncated_text}"`

- `source` is the vault-relative path (`p.source_file.display()`)
- `line_start` and `line_end` are the 1-indexed inclusive range from `ParagraphData`
- `truncated_text` is the first 60 characters of `p.text`, with `…` appended if truncated

Example: `Areas/finance.md:42-45  Revenue grew 12% YoY driven by new enterprise accounts…`

**Why 60 chars**: Long enough to identify a paragraph's topic at a glance, short enough not to crowd the tree view. This is a hardcoded constant in the TUI function — no configuration needed for v1.

### 2. Task status prefix: checkbox-style markers

Status mapping (from `TaskData.status` string to marker):

| Status string | Marker | Meaning |
|--------------|--------|---------|
| `"Open"` | `[ ]` | Todo |
| `"Done"` | `[x]` | Completed |
| `"InProgress"` | `[/]` | In progress |
| `"Cancelled"` | `[-]` | Cancelled |
| anything else | `[ ]` | Treat as open (defensive) |

Format: `"{marker} {description}"`

Example: `[x] Send quarterly report to stakeholders`

**Why checkbox style**: These match the Obsidian Tasks plugin visual convention that users already see in their notes. The mapping is deterministic and lives entirely in the TUI layer.

**Alternative considered**: Showing the raw status string like `[Done] Send report`. Rejected because it's verbose and doesn't match the `- [x]` style users see in markdown.

### 3. Both changes in `leaf_display` only

The `leaf_display` function is the single source of truth for tree-row display text. The fuzzy search picker (`GraphSearchPickerSource`) calls `leaf_display` to build candidate labels, so both the tree and the search picker benefit automatically. No other code paths need changes.

## Risks / Trade-offs

- **Line-end display could be noisy**: When start == end (single-line paragraphs), showing `42-42` is redundant. → Show `42` (just the line number) when start == end.
- **Task status string contract**: The `TaskData.status` spelling (`"Open"`, `"Done"`, `"InProgress"`, `"Cancelled"`) is a stable contract per the graph-task-nodes spec. The mapping in `leaf_display` is defensive — unknown values fall back to `[ ]`. → Low risk.
- **Wide tree rows**: Paragraph display is wider than before (was just `file:line`, now has range + text snippet). This could cause more horizontal scrolling in narrow terminals. → Acceptable; the information gain outweighs the width cost.
