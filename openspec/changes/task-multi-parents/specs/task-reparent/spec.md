## MODIFIED Requirements

### Requirement: Physical nesting strips the parent link

When `plan_move` targets `MoveTarget::UnderTask` with parent task P (including moves planned by `plan_indent` / `plan_outdent`), and P carries a `🆔` that the source's head line lists in its `⤴` field, that id SHALL be removed from the moved block's head line in the same plan. The whole field is removed if it was the only id. Other ids, other text on the line and every non-head line SHALL be left untouched. Moves to `Append` / `UnderHeading` targets SHALL NOT change references.

#### Scenario: Nest under one of two parents
- **WHEN** `journal.md:3` is `- [ ] reproduce ⤴ k3x9qa,ABC-123 📅 2026-10-23`, and it is moved under the `k3x9qa` task in `proj.md` as `Child`
- **THEN** the moved line in `proj.md` reads `<indent>- [ ] reproduce ⤴ ABC-123 📅 2026-10-23`

#### Scenario: Nest under an unrelated task keeps references
- **WHEN** the same task is moved under a task that isn't one of its parents
- **THEN** the moved line keeps `⤴ k3x9qa,ABC-123`, and the task now has three effective parents

#### Scenario: Move to a file keeps references
- **WHEN** the same task is moved with `--to inbox.md`
- **THEN** the moved line still contains `⤴ k3x9qa,ABC-123`
