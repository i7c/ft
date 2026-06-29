## MODIFIED Requirements

### Requirement: Directory rename updates all external references to contained files

The system SHALL update every vault-wide reference (wikilink, markdown link, embed) that points to any note under the renamed directory. References that use path-form wikilinks (`[[old-dir/file]]`) SHALL be updated to the new path; bare wikilinks (`[[file]]`) SHALL remain unchanged since the title (stem) does not change. Reference discovery SHALL iterate the unified link kinds (`NoteLink`, and — for completeness of byte-precise rewriting — the per-occurrence `LinkEdge.byte_range` they carry). Embed references (`LinkEdge.is_embed = true`) SHALL be updated identically to non-embed references. Anchored references (`[[old-dir/file#Section]]`) SHALL have their target path updated while preserving the `#anchor` portion.

#### Scenario: External note links to a file in the renamed directory

- **WHEN** renaming directory `docs/` to `reference/` and file `external.md` contains `[[docs/guide]]`
- **THEN** after the rename, `external.md` contains `[[reference/guide]]`

#### Scenario: Bare wikilink to a file in the renamed directory stays unchanged

- **WHEN** renaming directory `docs/` to `reference/` and file `external.md` contains `[[guide]]` (the title of `docs/guide.md`)
- **THEN** after the rename, `external.md` still contains `[[guide]]` (the title didn't change)

#### Scenario: Anchored reference preserves the anchor

- **WHEN** renaming directory `docs/` to `reference/` and file `external.md` contains `[[docs/guide#Section]]`
- **THEN** after the rename, `external.md` contains `[[reference/guide#Section]]` (path updated, anchor preserved)

#### Scenario: Embed reference updated

- **WHEN** renaming directory `docs/` to `reference/` and file `external.md` contains `![[docs/guide]]`
- **THEN** after the rename, `external.md` contains `![[reference/guide]]`

### Requirement: Directory rename handles cross-references within the renamed directory

The system SHALL correctly update references between files that are both being renamed. Edits SHALL be computed against old file paths, applied to files at their old paths, and then files SHALL be renamed to new paths (edit-then-rename).

#### Scenario: Two files in the renamed directory link to each other

- **WHEN** renaming directory `old/` to `new/` and `old/a.md` contains `[[old/b]]` and `old/b.md` contains `[[old/a]]`
- **THEN** after the rename, `new/a.md` contains `[[new/b]]` and `new/b.md` contains `[[new/a]]`
