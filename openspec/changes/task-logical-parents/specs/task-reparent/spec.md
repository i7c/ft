## ADDED Requirements

### Requirement: Physical nesting strips the parent link

When `plan_move` targets `MoveTarget::UnderTask` (including moves planned by `plan_indent` / `plan_outdent`), and a source's `expected` task carries a `parent_link` in any state, the raw text of that parent link, plus one adjacent space, SHALL be removed from the moved block's head line in the same plan. Other links on the line, and every non-head line, SHALL be left untouched. Moves to `Append` / `UnderHeading` targets SHALL NOT strip links.

#### Scenario: Paste a linked journal task under a project task
- **WHEN** `journal.md:3` is `- [ ] reproduce [[proj#^vo1]] 📅 2026-10-23` and it is moved under `proj.md:5` as `Child`
- **THEN** the moved line in `proj.md` reads `<indent>- [ ] reproduce 📅 2026-10-23`

#### Scenario: Move to a file keeps the link
- **WHEN** the same task is moved with `--to inbox.md`
- **THEN** the moved line still contains `[[proj#^vo1]]`

### Requirement: Cross-file moves retarget block references

`task::links::plan_retarget(plan, scan, ctx)` SHALL extend a `MovePlan` whose moved blocks change file, adding edits so that every wikilink or embed in the vault that resolved to `<source file>#^id` (for an `id` carried by any line in a moved block) points at the target file instead.

- The rewritten link keeps its anchor, display alias and embed marker, and uses the link-text formatting rules of `task-link-cli`.
- For files that already have an edit in the plan, the rewrite SHALL apply to that edit's `new` content: links are re-extracted from it, never located by stale byte offsets.
- Links inside moved blocks that pointed at the source file with `[[#^id]]` (same file) SHALL be rewritten to the explicit form when they end up in a different file from their target, and *vice versa*.

`ft tasks move` (with `--to` or `--under`), the TUI paste (`p`/`P`) and the TUI `M` move SHALL call `plan_retarget` before applying.

#### Scenario: Parent moved to another file
- **WHEN** `proj.md` contains `- [ ] Vendor onboarding ^vo1`, `journal.md` contains `- [ ] reproduce [[proj#^vo1]]`, and the user moves the parent with `ft tasks move proj.md:1 --to archive/2026.md`
- **THEN** `journal.md` now contains `[[2026#^vo1]]` (or the path form if `2026` is ambiguous), and the journal task's parent still resolves after a rescan

#### Scenario: Dry run shows the retarget
- **WHEN** the same move runs with `--dry-run`
- **THEN** the diff includes the `journal.md` link rewrite, and no file is written
