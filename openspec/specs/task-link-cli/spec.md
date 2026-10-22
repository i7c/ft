# task-link-cli Specification

## Purpose
CLI surface for logical parents: `ft tasks link`, `ft tasks unlink`, `ft
tasks check` and `ft tasks create --link-parent`, plus block-id generation
and link-text formatting. Created by change task-logical-parents.

## Requirements

### Requirement: Link planner

`task::links::plan_link(children, parent, ctx, format) -> Result<LinkPlan, LinkError>` SHALL build a pure plan (file edits, applied through `write_atomic` by `apply_link_plan`). Each child and the parent are line-addressed with an expected-`Task` guard, and `ctx` gives the vault root plus a note lookup for link formatting. The plan SHALL:

1. **Ensure the parent has a block id.** If the parent has no `block_link`, generate one and set it on the parent line.
2. **Rewrite each child's description.** If the child already has a parent link, its raw text is removed. The new link is then appended to the end of the description, so emoji fields stay after it.
3. **Reject cycles.** A child that is the parent itself, or an effective ancestor of the parent, fails with `LinkError::WouldCycle`.
4. **Reject physically nested children.** A child that already has a physical parent fails with `LinkError::PhysicallyNested`. Linking would be ignored, so the user is told to outdent it first.
5. **Guard every line.** Stale lines fail with `LineChanged` for a child or `TargetChanged` for the parent.

Children in the same file as the parent, or as each other, SHALL be applied in a single edit of that file.

#### Scenario: Link a journal task under a project task
- **WHEN** `proj.md:1` is `- [ ] Vendor onboarding` (no block id) and the user links `journal/2026-10-21.md:3` under it
- **THEN** `proj.md:1` becomes `- [ ] Vendor onboarding ^<id>`, and the journal line's description gains ` [[proj#^<id>]]` before its emoji fields

#### Scenario: Relink replaces the old link
- **WHEN** a task already contains `[[old#^o1]]` as its parent link and is linked under a different task
- **THEN** `[[old#^o1]]` is removed and the new link is appended

#### Scenario: Link under own descendant refused
- **WHEN** the user links task A under a task whose effective ancestors include A
- **THEN** the planner returns `WouldCycle` and nothing is written

### Requirement: Link text formatting

The generated link SHALL be:

- `[[#^id]]` when the child and the parent share a file;
- otherwise `[[<title>#^id]]` when the parent note's title (file stem) is unique in the vault;
- otherwise `[[<vault-relative path without .md>#^id]]`.

#### Scenario: Ambiguous title uses the path
- **WHEN** both `a/proj.md` and `b/proj.md` exist and the parent lives in `b/proj.md`
- **THEN** the link is written as `[[b/proj#^<id>]]`

### Requirement: Block id generation

Generated block ids SHALL be 6 lowercase base-36 characters, derived from a hash of the parent's path, line, description and the current time. They SHALL be re-derived (with a counter mixed in) until they are unique among all `^id`s in the parent's file. No new dependency is introduced.

#### Scenario: Collision avoided
- **WHEN** the derived id already exists in the parent's file
- **THEN** a different id is generated, and the written id is unique within the file

### Requirement: Unlink planner

`task::links::plan_unlink(task, format)` SHALL remove the task's parent-link raw text from its description, along with one adjacent space. Tasks with no parent link fail with `LinkError::NoParentLink`. The parent's block id SHALL be left in place.

#### Scenario: Unlink
- **WHEN** a task's line is `- [ ] reproduce [[proj#^vo1]] 📅 2026-10-23` and the user unlinks it
- **THEN** the line becomes `- [ ] reproduce 📅 2026-10-23`

### Requirement: `ft tasks link` and `ft tasks unlink`

The CLI SHALL provide:
- `ft tasks link <SELECTOR>... --under <SELECTOR>` (each child selector and the parent selector must match exactly one task);
- `ft tasks unlink <SELECTOR>` (must match exactly one task).

Both SHALL support `--dry-run` (unified diff) and `--format text|json`. The JSON form SHALL be an array of `{"description", "path", "line", "parent": {"path", "line"} | null}`, one entry per changed child, with vault-relative paths and post-edit lines. Errors SHALL use vault-relative paths and honour `--json-errors`.

#### Scenario: Link from the CLI
- **WHEN** the user runs `ft tasks link journal/2026-10-21.md:3 --under "Vendor onboarding"`
- **THEN** the task is linked as described by the link planner and the command prints `Linked 1 task(s) under proj.md:1`

#### Scenario: Ambiguous parent
- **WHEN** `--under` matches several tasks
- **THEN** the command fails listing the candidates, and nothing is written

### Requirement: `ft tasks create --link-parent`

`ft tasks create` SHALL accept `--link-parent <SELECTOR>`, which must match exactly one task. The task is created at its normal target (the daily note, `--file`, `--under-heading`, `--at-line`, `--append` and default-section rules all still apply), with the parent link appended to its description and a block id added to the parent if it lacks one, both in the same run. `--link-parent` SHALL conflict with `--parent`.

#### Scenario: Subtask in today's note under a remote anchor
- **WHEN** `jira/ABC-123.md` is an anchor note for `ABC-123`, and the user runs `ft tasks create "write regression test" --link-parent ABC-123`
- **THEN** today's daily note gains `- [ ] write regression test [[ABC-123]]`, using the anchor-note form because the parent is an anchor note's anchor task, and the task's effective parent is the anchor task

### Requirement: Anchor-note link preference

When the parent task is the anchor task of an anchor note (the note's `ft.tasks.anchor` equals the task's `🆔`), the link planner and `create --link-parent` SHALL write the anchor-note form `[[<title or path>]]` instead of a block reference, and SHALL NOT add a block id.

#### Scenario: No block id on anchor tasks
- **WHEN** a task is linked under the anchor task of `jira/ABC-123.md`
- **THEN** the child gains `[[ABC-123]]` and the anchor line is unchanged

### Requirement: `ft tasks check`

`ft tasks check` SHALL report every structural issue found by the scan:
- dangling parent links;
- links marked `Cycle`;
- `Ignored` links (physically nested and linked);
- extra parent links.

Each issue SHALL list the file, line, task description and raw link text. With `--format json` the output SHALL be an array of `{"kind": "dangling"|"cycle"|"ignored"|"extra", "path", "line", "description", "link"}`. The command SHALL exit 0 when there are no issues and 1 otherwise.

#### Scenario: Clean vault
- **WHEN** no task has a structural issue
- **THEN** `ft tasks check` prints `no issues` and exits 0

#### Scenario: Dangling link reported
- **WHEN** a task links `[[proj#^gone]]` and no `^gone` exists in `proj.md`
- **THEN** `ft tasks check --format json` outputs an entry with `"kind": "dangling"` for that task and exits 1
