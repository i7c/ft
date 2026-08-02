# tasks-create-parent design

## Context

`ft tasks create` (in `ft/src/cmd/tasks.rs`) currently places a new task in
a target file at one of: default section/append (`ops::auto_position`),
`--under-heading`, `--at-line`, or `--append`. It never scans the vault —
`run_create` only resolves the target path from `--file` or the daily note.

The subtask placement machinery already exists in `ft-core`:
`ops::Position::Subtask { parent_line }` derives the child indentation
(match first existing child, else parent indent + two spaces) and the
insertion line (end of the parent's indented block), then splices as an
ordinary `AtLine` write through `fs::write_atomic`. It is used by the TUI
quickline (`ft/src/tui/tabs/tasks/search.rs`) and tested there.

Task selection uses `ft_core::selector` (`parse` + `resolve`) over the full
vault scan. `Task.source_file` in a scan is vault-relative, so the parent's
absolute path is `vault.path.join(source_file)` — exactly what the TUI
quickline does for its subtask mode.

Constraints from AGENTS.md: the CLI is the contract (editor-agnostic,
machine-readable); mutations go through `ops::*` planners/`write_atomic`;
no new ft-core surface should be invented when the ops layer already covers
it (`ops::Position::Subtask` takes `&dyn TaskFormat` and is format-parametric).

## Goals / Non-Goals

**Goals:**
- `ft tasks create "…" --parent <SELECTOR>` writes an indented subtask at
  the end of the parent's block in the parent's file.
- Unique-match-or-error resolution: zero or multiple matches produce a hard
  error; multiple-match errors list candidates (mirroring the `--yes`
  non-TTY path of `complete`/`cancel`/`edit`).
- All three selector forms supported, including `file:line`; standard
  id→fuzzy fallback.
- `--parent` conflicts with `--file` / `--under-heading` / `--at-line` /
  `--append` at the clap layer.
- File-wide duplicate check (and `--force`) behaves exactly as today.

**Non-Goals:**
- No interactive picker for ambiguous parents (`create` is scriptable; the
  user asked for a hard error).
- No new ft-core API: `ops::Position::Subtask`, `selector`, and
  `subtask_placement` are reused as-is.
- No changes to the TUI quickline flow or its behavior.
- No recursive/multi-level creation beyond what indentation already gives
  (a subtask of a subtask works for free via `file:line` of the child).

## Decisions

### D1. Flag name and shape: `--parent <SELECTOR>` (string value)

Clap `#[arg(long, value_name = "SELECTOR")]`. The selector string is passed
to `selector::parse` at runtime — the same pipeline as `complete`/`cancel`/
`edit`. Alternatives considered: `--subtask-of` (longer, no benefit),
`--under` (collides conceptually with `--under-heading`). Confirmed with
the user: `--parent` it is.

### D2. Conflicts: `conflicts_with_all = ["file", "under_heading", "at_line", "append"]`

The parent determines both file and position; any other placement flag is
contradictory and errors at parse time (clap), before any vault work. This
matches option (a) confirmed with the user — strictest, no silent override.
Note clap's `file` conflict also covers the daily-note default: with
`--parent` the daily-note fallback is never reached because target comes
from the parent.

### D3. Resolution happens in `run_create` via a small helper

Add a private `fn resolve_parent(selector: &str, scan: &Scan) -> Result<&Task>`
(no lifetimes trouble: returns `Result<&'a Task>` over the scan). Logic:

1. `selector::parse(s)` → `sel`.
2. `selector::resolve(&scan.tasks, &sel)`.
3. If empty and `sel` is `Selector::Id`, retry with `Fuzzy` (existing
   fallback, matches `resolve_targets`).
4. Empty → `anyhow!("no tasks match selector `{s}`")`.
5. Multiple → error listing up to 5 candidates
   (`{file}:{line}  {description}` + `… and N more`), same shape as the
   `--yes` path of `pick_task`.

`run_create` scans the vault only when `--parent` is present (scan errors
warn via `tracing::warn!` as in the other commands), then sets
`target = vault.path.join(parent.source_file)` and
`position = Position::Subtask { parent_line: parent.source_line }`.

### D4. Reuse `ops::create_task` untouched

`create_task` already resolves `Subtask` → `AtLine` via `subtask_placement`
and runs the file-wide duplicate check against the target file, `--force`
bypassing it. Because the target file is the parent's file, the check
scans that file as today — identical semantics to the TUI quickline
(confirmed with the user: keep it). No `ft-core` change, no signature
ripple; the CLI is a thin adapter exactly like the TUI's subtask mode.

### D5. Done parents allowed via `id` / `file:line`

`selector::resolve` already restricts only `Fuzzy` to non-Done tasks.
Consistent with the other commands: no extra filtering in the helper.
Fuzzy text can't accidentally pick a done parent.

## Risks / Trade-offs

- **Stale line numbers**: a `file:line` parent selector resolves against a
  scan; the file could shift before the write. `create_task`'s splice is
  line-addressed after resolution (same window every mutation has).
  Mitigation: resolution and write are sequential in one process with no
  await points; the window is the existing, accepted one for all
  line-addressed ops. `subtask_placement` bounds-checks the line and
  returns `LineOutOfRange` on a stale file.
- **Ambiguity UX for fuzzy parents**: a short fuzzy needle may match many
  tasks. Mitigation: error lists candidates (`file:line` + description),
  which doubles as a hint to switch to `file:line` or id.
- **Duplicate check surprises**: a same-described+dated task elsewhere in
  the parent's file blocks subtask creation without `--force`.
  Mitigation: pre-existing semantics, matches TUI quickline; error already
  prints the duplicate's `file:line` and suggests `--force`.

## Migration Plan

Purely additive CLI flag; no config, no data migration, no deprecations.
Rollback is removing the flag. ft.nvim gains nothing mandatory — pass-through
only, coordinated later if the plugin wants a `parent` opt.

## Open Questions

None outstanding — the three disambiguation questions (flag name,
`--file` interaction, duplicate-check scope) are resolved in D1/D2/D4.
