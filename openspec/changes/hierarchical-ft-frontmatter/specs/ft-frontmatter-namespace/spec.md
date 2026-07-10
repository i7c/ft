## ADDED Requirements

### Requirement: Hierarchical ft frontmatter namespace

ft-owned frontmatter keys SHALL live under a single `ft:` YAML map.
The four legacy flat keys map to nested keys as follows:

| Legacy flat key        | New nested key            |
|------------------------|---------------------------|
| `ft-tasks-section`     | `ft.tasks.section`        |
| `ft-append-section`    | `ft.append.section`       |
| `ft-synth: true`       | `ft.synth.enabled: true`  |
| `ft-synth-targets`     | `ft.synth.targets`        |

The `ft:` map and its sub-keys SHALL be optional; notes with no
frontmatter, or frontmatter lacking an `ft:` map, SHALL behave
exactly as notes with no ft keys today.

#### Scenario: Nested form is read

- **WHEN** a note's frontmatter contains `ft:\n  tasks:\n    section: Tasks\n` and `ft tasks` resolves a target position without an explicit override
- **THEN** the new task SHALL land under the `Tasks` heading

#### Scenario: No ft map behaves as today

- **WHEN** a note has frontmatter with only `title: Foo` and no `ft:` map
- **THEN** all ft features treat the note as having no ft keys (tasks append to end, not a synth note, etc.)

### Requirement: Legacy flat keys are not recognized

Readers SHALL recognize only the nested `ft:` keys. The legacy flat
keys (`ft-tasks-section`, `ft-append-section`, `ft-synth`,
`ft-synth-targets`) SHALL be treated as ordinary unknown frontmatter
and SHALL NOT trigger any ft behavior. This is a breaking change for
existing vaults carrying flat keys, accepted as a one-time exception
to the project's vault-data compatibility rule.

#### Scenario: Legacy flat synth marker is ignored

- **WHEN** a note's frontmatter contains `ft-synth: true` (legacy flat form) and no nested `ft:` map
- **THEN** `is_synth_note` SHALL return `false` and the note SHALL NOT be treated as a synth note

#### Scenario: Legacy flat tasks-section is ignored

- **WHEN** a note's frontmatter contains `ft-tasks-section: Tasks` (legacy flat form) and no nested `ft:` map
- **THEN** `ft tasks` SHALL resolve the new-task position to file-end (or the `[tasks] default_section` config), ignoring the legacy value

### Requirement: Writers emit nested form only and clean up orphans

Every ft code path that writes frontmatter SHALL emit the nested `ft:`
form, not the legacy flat keys. This covers `upsert_synth_frontmatter`
and the synth scaffold's fresh-note frontmatter. When a writer rewrites
the frontmatter of a note that contains legacy flat `ft-*` keys owned
by the same feature, the writer SHALL remove those legacy lines (orphan
cleanup) so the note is left in the canonical nested form with no dead
flat keys. `upsert_synth_frontmatter` SHALL remove legacy `ft-synth:`
and `ft-synth-targets:` lines; it SHALL NOT touch unrelated frontmatter
(including legacy `ft-tasks-section` / `ft-append-section`, which belong
to other features).

#### Scenario: Scaffold creates a note with nested frontmatter

- **WHEN** `ft notes synth scaffold Synthesis/topic.md --link "[[Foo]]"` creates a new note
- **THEN** the file's frontmatter SHALL contain `ft:\n  synth:\n    enabled: true\n    targets: ["[[Foo]]"]` and SHALL NOT contain any `ft-synth` or `ft-synth-targets` flat key

#### Scenario: Upsert on a legacy note strips the legacy keys

- **WHEN** `upsert_synth_frontmatter` is applied to a note whose frontmatter has `ft-synth: true` (legacy), `ft-synth-targets: ["[[Old]]"]` (legacy), and `title: Foo`
- **THEN** the result SHALL contain the nested `ft.synth.enabled: true` (and the supplied targets, if any), SHALL preserve `title: Foo`, and SHALL NOT contain the legacy `ft-synth:` or `ft-synth-targets:` lines

#### Scenario: Upsert preserves unrelated frontmatter keys

- **WHEN** `upsert_synth_frontmatter` is applied to a note whose frontmatter has `title: My Note` and `tags: [a]`
- **THEN** the result retains `title` and `tags` unchanged alongside the nested `ft.synth.enabled: true` and `ft.synth.targets`
