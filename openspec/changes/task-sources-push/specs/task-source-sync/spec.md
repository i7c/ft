## ADDED Requirements

### Requirement: Stored conflicts

When the status rules find a conflict (local, base and remote all differ), the planner SHALL write `ft.source.conflict: <remote Status>` into the note and keep the local status, as before. On a later sync:
- **Remote agrees:** if the remote status equals the local status, the key SHALL be removed and `synced.status` updated.
- **Remote changes again:** the key SHALL be updated to the new remote status.

A conflicted note SHALL NOT count as a pending push.

#### Scenario: Conflict recorded
- **WHEN** base is `InProgress`, local is `Done`, and remote is `Cancelled`
- **THEN** the note gains `ft.source.conflict: Cancelled`, and the anchor stays `[x]`

### Requirement: Local conflict resolution

`sources::resolve_conflict(note, Take::Local | Take::Remote)` SHALL be a pure planner over the note's content:
- **Take remote:** set the anchor's status to the stored remote status, with `✅` / `❌` dated by the sync date when entering Done or Cancelled; set `synced.status` to it; remove `conflict`.
- **Take local:** set `synced.status` to the stored remote status and remove `conflict`. The anchor keeps its local status, which makes it a pending push.

It SHALL touch no other line, and SHALL need no network.

#### Scenario: Take local makes it pending
- **WHEN** a note has local `Done`, `synced.status: InProgress` and `conflict: Cancelled`, and the user takes local
- **THEN** `synced.status` becomes `Cancelled`, `conflict` is removed, and the next sync pushes Done

### Requirement: Check reports conflicts and push failures

`ft tasks check` SHALL also report:
- `conflict` for every issue note with `ft.source.conflict`, with link text `local=<x> remote=<y>`;
- `push-failed` for every cached push failure, with the reason as the link text.

These count toward the exit status.

#### Scenario: Check lists a conflict
- **WHEN** `jira/ABC-123.md` has a stored conflict
- **THEN** `ft tasks check --format json` includes `{"kind": "conflict", "path": "jira/ABC-123.md", …}` and exits 1
