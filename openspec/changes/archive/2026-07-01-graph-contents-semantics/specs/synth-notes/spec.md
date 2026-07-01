## MODIFIED Requirements

### Requirement: Scaffold content sourcing

With `--link` flags, the scaffold SHALL be sourced from the multi-source journal for the selected links over the specified window. With `--in-window`, only paragraphs whose lines overlap added-lines in the window SHALL be included. With `--all` (the default) or no window flag, all-time matching paragraphs SHALL be included. With `--from <path>:<line>` (repeatable), the scaffold SHALL additionally include the specified source paragraphs (identified by the line in which they start). Sections in the resulting scaffold SHALL be ordered by journal date descending (newest first), preserving the journal's tiebreak (source title ascending) for equal dates.

The scaffold's per-section body text SHALL be taken verbatim from `JournalEntry.section_text`, which derives from `ParagraphData.text`. Because the heading line remains part of the paragraph that begins at that line (Fork A2), `section_text` is unchanged in shape: a paragraph that begins at a heading line still includes the heading line verbatim.

#### Scenario: --link sources from journal
- **WHEN** `ft synth out.md --link "[[Foo]]" --link "[[Bar]]"` is run
- **THEN** the scaffold includes a section for every paragraph that the multi-source journal returns for `Foo` or `Bar`

#### Scenario: --in-window filter applied
- **WHEN** `ft synth out.md --link "[[Foo]]" --since 7d --in-window` is run
- **THEN** only paragraphs whose lines overlap added-lines in the last 7 days are included

#### Scenario: --from picks specific paragraphs
- **WHEN** `ft synth out.md --link "[[Foo]]" --from notes/bar.md:42 --no-edit` is run
- **THEN** the scaffold includes the journal results for `[[Foo]]` PLUS the paragraph starting at line 42 of `notes/bar.md`

#### Scenario: Scaffold ordered newest first
- **WHEN** the scaffold contains paragraphs dated 2026-03-01 and 2025-11-14
- **THEN** the 2026-03-01 section appears before the 2025-11-14 section in the file

#### Scenario: Paragraph beginning at a heading line includes the heading
- **WHEN** a sourced paragraph begins at a `## Section` heading line
- **THEN** the scaffolded callout body begins with `## Section` (the heading line is part of `ParagraphData.text`, per Fork A2)
