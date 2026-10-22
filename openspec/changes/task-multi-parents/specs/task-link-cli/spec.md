## ADDED Requirements

### Requirement: Task id generation

When a task becomes a parent and has no `🆔`, the planner SHALL generate one: 6 lowercase base-36 characters, derived from a hash of the task's path, line, description and the current time. The id SHALL be re-derived (with a counter mixed in) until it is absent from the vault-wide `🆔` index and from every id already generated in the same plan. A task that already has a `🆔` (user-chosen, or an issue key) SHALL keep it. No new dependency is introduced.

#### Scenario: Collision avoided vault-wide
- **WHEN** the derived id already exists on a task in another file
- **THEN** a different id is generated, and the written id is unique in the vault

#### Scenario: Existing id reused
- **WHEN** the parent line already carries `🆔 vendor-onb`
- **THEN** the child gains `⤴ vendor-onb` and the parent line is unchanged

## MODIFIED Requirements

### Requirement: Link planner

`task::links::plan_link(children, parent, ctx, format) -> Result<LinkPlan, LinkError>` SHALL build a pure plan (file edits, applied through `write_atomic` by `apply_link_plan`). Each child and the parent are line-addressed with an expected-`Task` guard. The plan SHALL:

1. **Ensure the parent has a `🆔`.** If the parent has none, generate one (see "Task id generation") and set it on the parent line.
2. **Add the parent to each child.** The parent's id is appended to the child's `parent_ids`, and the line is re-serialized through the format, leading whitespace kept. A child that already lists the id is left unchanged and reported as `unchanged`.
3. **Reject cycles.** A child that is the parent itself, or an effective ancestor of the parent, fails with `LinkError::WouldCycle`.
4. **Reject redundant links.** A child whose physical parent is the parent fails with `LinkError::AlreadyPhysicalParent`.
5. **Guard every line.** Stale lines fail with `LineChanged` for a child or `TargetChanged` for the parent.

A child's existing references SHALL be kept; linking only adds. Children in the same file as the parent, or as each other, SHALL be applied in a single edit of that file.

#### Scenario: Link a journal task under a project task
- **WHEN** `proj.md:1` is `- [ ] Vendor onboarding` (no `🆔`) and the user links `journal/2026-10-21.md:3` (`- [ ] reproduce 📅 2026-10-23`) under it
- **THEN** `proj.md:1` becomes `- [ ] Vendor onboarding 🆔 <id>`, and the journal line becomes `- [ ] reproduce ⤴ <id> 📅 2026-10-23`

#### Scenario: Second parent is added
- **WHEN** a task already has `⤴ k3x9qa` and is linked under the task with `🆔 ABC-123`
- **THEN** the task's field becomes `⤴ k3x9qa,ABC-123`

#### Scenario: Linking twice changes nothing
- **WHEN** a task with `⤴ k3x9qa` is linked under the `k3x9qa` task again
- **THEN** no file is written and the child is reported as unchanged

#### Scenario: Link under own descendant refused
- **WHEN** the user links task A under a task whose effective ancestors include A
- **THEN** the planner returns `WouldCycle` and nothing is written

### Requirement: Unlink planner

`task::links::plan_unlink(task, which, format)` SHALL remove line references from the task's `parent_ids`, where `which` is one of:
- `Only`: the task's single line reference. With none it fails with `LinkError::NoParentLink`; with more than one it fails with `LinkError::NeedsChoice { refs }`, listing each reference's id and resolved parent (path, line, description) when resolved.
- `Parent(TaskKey)`: every line reference resolving to that task. With none it fails with `LinkError::NotLinkedTo`. If that parent comes only from the anchor note's frontmatter, it fails with `LinkError::ParentFromFrontmatter`.
- `All`: every line reference. With none it fails with `NoParentLink`.

Removing the last id SHALL remove the whole `⤴` field. The parent's `🆔` SHALL be left in place.

#### Scenario: Unlink one of two
- **WHEN** a task's line is `- [ ] reproduce ⤴ k3x9qa,ABC-123 📅 2026-10-23` and the user unlinks it from the `ABC-123` anchor task
- **THEN** the line becomes `- [ ] reproduce ⤴ k3x9qa 📅 2026-10-23`

#### Scenario: Unlink the last reference
- **WHEN** a task's line is `- [ ] reproduce ⤴ k3x9qa 📅 2026-10-23` and the user unlinks it with `Only`
- **THEN** the line becomes `- [ ] reproduce 📅 2026-10-23`

#### Scenario: Choice needed
- **WHEN** a task has two line references and `Only` is requested
- **THEN** the planner returns `NeedsChoice` with both references and nothing is written

### Requirement: `ft tasks link` and `ft tasks unlink`

The CLI SHALL provide:
- `ft tasks link <SELECTOR>... --under <SELECTOR>`. Each child selector and the parent selector must match exactly one task. A bare `🆔` value is a valid selector.
- `ft tasks unlink <SELECTOR> [--from <SELECTOR> | --all]`. The task selector must match exactly one task. `--from` maps to `Parent`, `--all` to `All`, and neither to `Only`.

Both SHALL support `--dry-run` (unified diff) and `--format text|json`.

- **JSON:** an array of `{"description", "path", "line", "id", "parents": [{"id", "path", "line", "state", "origin"}], "unchanged": bool}`, one entry per named child, with vault-relative paths and post-edit lines.
- **`NeedsChoice`:** exits 2 and lists the references. With `--json-errors`, the error object carries a `refs` array of the same shape as `parents`.
- **Errors:** use vault-relative paths and honour `--json-errors`.

#### Scenario: Link from the CLI
- **WHEN** the user runs `ft tasks link journal/2026-10-21.md:3 --under "Vendor onboarding"`
- **THEN** the task is linked as described by the link planner and the command prints `Linked 1 task(s) under proj.md:1`

#### Scenario: Unlink needs a choice
- **WHEN** the task has `⤴ k3x9qa,ABC-123` and the user runs `ft tasks unlink journal.md:3`
- **THEN** the command exits 2 listing both parents and suggesting `--from` or `--all`, and nothing is written

#### Scenario: Ambiguous parent selector
- **WHEN** `--under` matches several tasks
- **THEN** the command fails listing the candidates, and nothing is written

### Requirement: `ft tasks create --link-parent`

`ft tasks create` SHALL accept `--link-parent <SELECTOR>`, repeatable, with each selector matching exactly one task. The task is created at its normal target (the daily note, `--file`, `--under-heading`, `--at-line`, `--append` and default-section rules all still apply), with a `⤴` field listing the parents' ids in flag order. Any parent without a `🆔` gets one, in the same run. `--link-parent` SHALL conflict with `--parent`.

#### Scenario: Subtask of a ticket and a local goal
- **WHEN** `jira/ABC-123.md`'s anchor carries `🆔 ABC-123`, `Ship Q4 goal` has no `🆔`, and the user runs `ft tasks create "write regression test" --link-parent ABC-123 --link-parent "Ship Q4 goal"`
- **THEN** `Ship Q4 goal` gains `🆔 <id>`, today's daily note gains `- [ ] write regression test ⤴ ABC-123,<id>`, and the task has both as effective parents

### Requirement: `ft tasks check`

`ft tasks check` SHALL report every structural issue found by the scan:
- `dangling`: a reference whose id no task carries;
- `ambiguous`: a reference whose id more than one task carries;
- `cycle`: a reference marked `Cycle`;
- `redundant`: a line reference naming the task's physical parent, or the same id listed twice;
- `duplicate-id`: every task after the first (in `(path, line)` order) that carries an already-used `🆔`.

Each issue SHALL list the file, line, task description and the id involved. With `--format json` the output SHALL be an array of `{"kind", "path", "line", "description", "id"}`. Source issues (conflicts, push failures) keep their existing entries. The command SHALL exit 0 when there are no issues and 1 otherwise.

#### Scenario: Clean vault
- **WHEN** no task has a structural issue
- **THEN** `ft tasks check` prints `no issues` and exits 0

#### Scenario: Copy-pasted line
- **WHEN** `a.md:1` and `b.md:4` both carry `🆔 k3x9qa`, and `c.md:2` has `⤴ k3x9qa`
- **THEN** `check --format json` contains a `duplicate-id` entry for `b.md:4` and an `ambiguous` entry for `c.md:2`, and exits 1

## REMOVED Requirements

### Requirement: Block id generation
**Reason**: Parents are referenced by vault-wide `🆔`, not by per-file block ids. See "Task id generation".
**Migration**: None. Existing `^id`s stay valid as ordinary block references, and cross-file moves still retarget them.

### Requirement: Anchor-note link preference
**Reason**: There is no link text to choose any more. Anchor tasks are referenced by their `🆔` (the issue key) like any other task.
**Migration**: None.
