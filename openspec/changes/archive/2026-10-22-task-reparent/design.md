## Context

A task in ft is one Tasks-plugin emoji line. Its identity is
`TaskKey = (file, line)` (`task/hierarchy.rs:8`). Parent/child comes only
from indentation within one file: `resolve_hierarchy` assigns each task to
the nearest preceding task with a smaller indent (`hierarchy.rs:240`).

The move machinery is already a clean plan/apply pair:

- `ops::plan_move(&[MoveSource], &MoveTarget, &dyn TaskFormat) -> MovePlan`
  groups sources by file. For each source it computes the block
  (`block_end`: every following line indented deeper than the head; blank
  lines end the block), drops blocks contained in other blocks, strips
  each block's indentation to column 0 (`strip_indent`), and splices the
  result into the target (`splice_into_target`). It also handles the case
  where source and target are the same file.
- `apply_move_plan` writes each changed file through `write_atomic`.
- `MoveTarget` today is `Append(path)` or `UnderHeading(path, heading)`.
- Subtask creation (`Position::Subtask`, `subtask_placement`) finds the
  end of the parent's block and indents to match the first child, or the
  parent plus two hard-coded spaces.

The real vault (`nunotes`) nests with 4 spaces, so the hard-coded
2-space fallback already gives inconsistent indentation there.

On the TUI Tasks tab:
- There is one cursor and no multi-select.
- `M` opens a file/heading picker (`ActiveModal::TaskMove`, `tabs/tasks/modals.rs`).
- `p`/`P` cycle priority and `x`/`X` complete or cancel.
- `d`, `>`, `<`, `+`, `-` and `Esc` are unbound (`search.rs:54`).

## Goals / Non-Goals

**Goals:**
- Move a task (with its subtree) under any other task, or right after
  it as a sibling, within a file or across files, with the same guards
  and atomic writes as existing moves.
- Indent and outdent one level.
- A fast TUI flow: mark tasks with `d`, then press `p`/`P` on a target.
  No picker.
- Indentation that matches the target file.
- A CLI surface that editor clients can drive (`--format json`).

**Non-Goals:**
- Logical (link-based) parents across files. That's phase 2. In this
  change a parent is always physically above its children in the same
  file.
- Remote sources (Jira/GitHub). Phases 3–4.
- Reordering siblings upward ("move above"). Not requested; `SiblingAfter`
  combined with `Child` covers placement.
- A cut set that persists across TUI restarts.
- Rewriting wikilinks on cross-folder moves. This keeps the existing
  `TODO(plan-003)`.

## Decisions

### D1. Extend `MoveTarget`; don't add a parallel planner

```rust
pub enum MoveTarget {
    Append(PathBuf),
    UnderHeading(PathBuf, String),
    UnderTask { path: PathBuf, line: usize, expected: Option<Task>, placement: Placement },
}
pub enum Placement { Child, SiblingAfter }
```

`plan_move` already solves the hard parts: grouping, dropping contained
blocks, same-file removal followed by insertion, and expected guards.
The only new work is in the insertion step. Instead of splicing at the
file end or at a section end, it splices at `block_end(target) + 1`
with re-indentation.

Considered: a separate `plan_reparent`. Rejected because it would
duplicate the removal logic and diverge over time.

### D2. Resolve the insertion point before removals, then map it through them

When the target file is also a source file, `plan_move` removes the
source blocks first. The target's original index `t` is computed on the
original lines, together with its block end `e`. The post-removal index
is then `e - |removed lines with index < e|`.

If `t` lies inside any removed range, the move would put a task inside
its own subtree, so `plan_move` returns `IntoOwnSubtree`. This check
also covers the trivial case of a task targeting itself.

`block_end(target)` is computed on the original lines. Any of the
target's descendants that are being moved away are already counted in
the removal mapping.

### D3. Re-indent instead of stripping to column 0

For `Append` and `UnderHeading`, the current strip-to-column-0
behaviour stays unchanged. For `UnderTask`:

1. Compute `new_head_ws`:
   - `Child`: the target's first existing child's leading whitespace,
     verbatim, if the target has children. Otherwise the target's
     whitespace plus one target indent unit.
   - `SiblingAfter`: the target's leading whitespace.
2. For each line in the block, `rel = leading_ws(line)[head_ws.len()..]`
   (the whitespace beyond the head's).
   - If the source and target units are equal, keep `rel` verbatim.
   - Otherwise, `depth = units_in(rel, src_unit)`, and the line is
     re-emitted as `target_unit * depth` plus any leftover whitespace.
3. Each line becomes `new_head_ws + rel' + trimmed_content`.

This preserves the user's own nesting while keeping files internally
consistent. The tab and space conversion only kicks in when the two
files actually differ.

### D4. `detect_indent_unit`

The function scans list-item lines (`-`, `*`, `+`, or `N.` after
whitespace) and works out each item's parent indent with a stack.
- If the first nested item starts with a tab, it returns `Tab`.
- Otherwise it returns `Spaces(min positive delta)`.
- With no nesting, it returns `Spaces(2)`.

It is cheap: it runs over the lines already read by `plan_move` or
`create_task`, with no extra I/O.

`subtask_placement` switches its fallback from `"  "` to the detected
unit. This is a small, deliberate behaviour change for
`ft tasks create --parent` and the TUI `tasks.new-subtask`. It is
captured as a MODIFIED delta of `tasks-create-parent`.

### D5. Indent and outdent are thin wrappers over `plan_move`

- `plan_indent(src)`: read the file, parse its tasks, and run
  `resolve_hierarchy`. Find the nearest preceding task with the same
  `parent` and the same `indent_level`, then build
  `UnderTask{prev, Child}`.
- `plan_outdent(src)`: look up `src.parent`, then build
  `UnderTask{parent, SiblingAfter}`.

The wrappers use the same apply path and the same guards (the source
snapshot is the `expected` guard; the target guard is the freshly read
line, so staleness is checked against the same read).

Outdent places the task after the parent's whole block. This is the
Workflowy/Logseq behaviour, which never silently adopts the following
siblings, rather than the Obsidian editor behaviour (shift-tab, which
leaves the line in place and re-parents the following siblings to it).

### D6. CLI surface

- `MoveArgs` gains `--under <SEL>` (in a clap `ArgGroup` with `--to`,
  required, mutually exclusive) and `--sibling` (requires `--under`).
- New subcommands `Indent { selector, dry_run, format }` and
  `Outdent { … }`.
- Selector resolution reuses the `--parent` resolver from
  `tasks-create-parent` (unique match, id→fuzzy fallback, candidate
  listing).

`--format json` is added to the three mutation commands. `MovePlan`
gains `MovedBlock::dest: Option<(PathBuf, usize)>`, which `plan_move`
fills in for every target kind (it already knows the splice index). The
CLI renders it relative to the vault.

### D7. TUI: a cut set on `SearchView`, not a modal

Cut and paste is not a capturing multi-step flow: the user keeps
navigating, filtering and even mutating while tasks are marked. A modal
would block all of that. So the cut set is plain tab state:
`cut: Vec<CutEntry { key: TaskKey, snapshot: Task }>` on `SearchView`,
rendered as a gutter marker. The modal-driver rule targets flows that
capture input; this one doesn't.

- `d` toggles; `Esc` clears (only when the set is non-empty, so `Esc`
  keeps its other meanings elsewhere).
- `p` builds `MoveSource`s in cut order with `expected: Some(snapshot)`
  and an `UnderTask{cursor, Child}` target guarded by the cursor's
  snapshot. It then runs plan, apply, `request_graph_refresh`, and sets
  a pending anchor on `blocks[0].dest`.
- `P` is the same with `SiblingAfter`.
- On a generation change (`on_graph_ready`), each entry is re-resolved
  by matching its serialized line in the same file, nearest line first.
  Entries that can't be resolved are dropped with a toast.

Considered: clearing the cut set on every refresh. Rejected, because
completing a recurring task (which inserts a line) would silently lose
the user's marks.

### D8. Keymap: priority moves to `+`/`-`

The user chose vim-like `p`/`P` for paste. `tasks.priority-next` and
`tasks.priority-prev` move to `+` and `-`; the command names are
unchanged, so `ft do` and `[keymap]` overrides keep working. This is
called out as BREAKING in the proposal and the README.

### D9. ft.nvim

The plugin stays a thin client. It runs
`ft tasks move <file>:<line> --under <file>:<line> [--sibling] --format json`
(the source is the line under the cursor, the target comes from a task
picker it already has), plus `indent`/`outdent` on the cursor line,
then reloads buffers and jumps to `to.path:to.line`. All the domain
logic stays in the CLI.

## Risks / Trade-offs

- **Keymap retraining (`p` no longer changes priority).** Mitigated by
  the proposal and README call-out. The `?` overlay shows the new
  chords.
- **Same-file index mapping bugs could corrupt files.** Mitigated by:
  - a table-driven test suite over (source before/after/inside target)
    × (Child/SiblingAfter) × (same/different file);
  - a property test that a move never changes the multiset of non-blank
    lines, apart from whitespace;
  - all writes going through `write_atomic`, and `--dry-run` showing
    diffs.
- **Indent-unit heuristic misfires on odd files** (e.g. 3-space
  continuation lines). The detector only considers list items, and
  existing children always win over detection.
- **Cut-set re-resolution can bind to a duplicate identical line.**
  Nearest-line matching makes this unlikely, and the move's
  `expected` guard still prevents moving something that doesn't match.
- **Blank line inside a subtree ends the block** (existing `block_end`
  semantics). This is inherited and unchanged. It's documented, because
  reparenting makes it more visible.

## Roadmap (phases 2–4, for context; separate changes)

Agreed with the user on 2026-10-21. Each phase is its own openspec
change, building on this one.

**Principles:** the vault is the source of truth; offline is a full
mode; the network only touches explicit or background sync, never
`scan`.

1. **Phase 2: logical parents (`task-logical-parents`).**
   - A task may name a parent in another file with a wikilink. Only
     links to (a) a task's block reference (`[[proj#^abc]]`) or (b) an
     issue note count. The first qualifying link wins, extra ones warn,
     and ordinary note links never count.
   - Linking to a task that has no block id gives it a `^id`
     automatically.
   - `hierarchy` becomes cross-file: indentation first, then links.
   - TUI: a "link under" paste variant keeps the child where it is and
     inserts the link. The DSL gains `parent`, `has_children` and
     `orphaned`.
   - Physical moves (this change) stay the default.
2. **Phase 3: source pull (`task-sources`).**
   - A `TaskSource` trait in `ft-core` with Jira and GitHub
     implementations, using blocking HTTP with timeouts (no async
     runtime), run by a TUI worker modelled on git-sync.
   - Config: `[sources.<name>] kind = "jira"|"github"` with named
     queries (JQL / GitHub search). The default is "assigned to me,
     open".
   - Parents and epics are fetched too.
   - Auth: Jira uses an API token in the macOS Keychain via
     `ft sources login jira` (`FT_JIRA_TOKEN` overrides). GitHub uses
     `gh auth token`.
   - Each issue becomes one note (`jira/<KEY>.md`,
     `github/<owner>-<repo>-<n>.md`, configurable, with an optional
     note template). The frontmatter holds `ft-source`, key, URL,
     status, assignee and `ft-parent`. The note has one anchor task
     line, `🆔 <KEY>`, and local subtasks nest under it.
   - Jira-owned fields (title, status, due, priority) are overwritten
     on sync. `ft tasks edit` and the TUI refuse to edit them, with a
     hint. Scheduled, start, tags and subtasks stay local.
   - A closed or unassigned issue gets its anchor marked done, and the
     note is kept.
   - Sync runs on TUI start, on a background interval and via
     `ft tasks sync`.
   - **Multi-machine:** the vault is synced over git to several
     machines, and not all of them can reach Jira. So all sync state
     lives in the vault, never in `.ft/cache`. The last-synced remote
     values are stored in the issue note's frontmatter (e.g.
     `ft-synced: {status, updated}`), which commits and merges along
     with the note. A machine without credentials skips sync silently
     and is otherwise fully functional.
3. **Phase 4: push (`task-sources-push`).**
   - Pending pushes are derived from the vault, not stored in a local
     outbox. If an anchor's checkbox status differs from
     `ft-synced.status`, that is a pending transition. Whichever
     machine can reach Jira next pushes it, using a configurable
     status → transition map. For GitHub, done closes the issue and
     reopening reopens it.
   - `ft tasks promote <sel>` turns a local subtask into a Jira
     sub-task or GitHub sub-issue. It first writes a pending issue note
     (`key: pending`), which the next connected sync materialises.
   - Conflicts (both sides changed status since `ft-synced`) stop the
     sync for that issue. They're listed and resolved with
     `ft tasks sync --resolve` or a TUI modal.

## Open Questions

None blocking phase 1.
