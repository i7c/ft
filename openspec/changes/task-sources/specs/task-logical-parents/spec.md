## ADDED Requirements

### Requirement: Anchor-note frontmatter parent

When a note is an anchor note (`ft.tasks.anchor` names a task in it) and its frontmatter also has `ft.tasks.parent` set to a wikilink string (e.g. `"[[EPIC-7]]"`), the anchor task's parent link SHALL be resolved from that value, using the same candidate rules as a parent link on a task line. The resulting `ParentLink.raw` SHALL be the frontmatter value, and states follow the same rules:
- `Ignored` when the anchor line is indented under another task;
- `Dangling` when the target doesn't resolve to a task;
- cycle handling as for line links.

A parent link on the anchor line itself SHALL take precedence over the frontmatter value; the frontmatter value then counts as an extra link. `plan_unlink` on a task whose parent comes from frontmatter SHALL fail with `LinkError::ParentFromFrontmatter`, rendered as `parent comes from the note's ft.tasks.parent — edit the frontmatter (or the issue in its source)`.

#### Scenario: Story under epic
- **WHEN** `jira/ABC-123.md` (anchor `ABC-123`) has `ft.tasks.parent: "[[EPIC-7]]"` and `jira/EPIC-7.md` is the anchor note for `EPIC-7`
- **THEN** `ABC-123`'s anchor task resolves its parent link to `EPIC-7`'s anchor task
