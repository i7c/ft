## Why

Paragraph and task nodes in the graph tree currently show minimal display text: paragraphs show only `file.md:line_start` (no content preview), and tasks show only the bare description (no status indicator). This makes it difficult to quickly identify paragraph contents or task completion state at a glance — you have to open the note to see what a paragraph actually says or whether a task is done.

## What Changes

- **Paragraph display**: Show `line_start-line_end` range plus the first 60 characters of paragraph text, separated by a space. Example: `Areas/finance.md:42-45  The quick brown fox jumps over the lazy dog...`
- **Task display**: Prefix the description with a checkbox-style status marker: `[ ]` (Open), `[x]` (Done), `[/]` (InProgress), `[-]` (Cancelled). Example: `[ ] Fix the login regression`
- The `leaf_display` function in the TUI graph tab is the only code affected — these are presentation-only changes.

## Capabilities

### New Capabilities

<!-- No new capability — this is a pure display enhancement within existing node types -->

### Modified Capabilities

- `graph-task-nodes`: The TUI rendering requirement is modified — task node display text SHALL include a checkbox-style status prefix in addition to the description.

## Impact

- **Affected code**: `ft/src/tui/tabs/graph.rs` — `leaf_display` function, match arms for `NodeKind::Paragraph` and `NodeKind::Task`.
- **APIs**: No core API changes. Display format changes stay in the TUI binary crate.
- **Snapshot tests**: TUI snapshots with paragraph and task rows will change. The in-tree search fuzzy picker (`GraphSearchPickerSource`) also uses `leaf_display` so its candidate labels will update automatically.
