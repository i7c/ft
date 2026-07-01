## MODIFIED Requirements

### Requirement: Paragraph-level frequency dedup

For each `[[Link]]` or `[markdown link](path.md)` occurrence on an added line, the command SHALL map the line to its containing paragraph using the HEAD-state paragraph index of the post-commit path (enumerated via `Graph::note_paragraphs`). It SHALL count distinct `(link, paragraph)` pairs. If the line cannot be mapped to a current paragraph (file deleted or paragraph rewritten such that no current paragraph contains the original added line), the command SHALL fall back to a synthetic key of `(path, original-added-line)` so the link is still counted but does not dedup against current paragraphs. Both wikilink and markdown-link occurrences on added lines SHALL be counted (the unified link kinds include both forms).

#### Scenario: Same link twice in one paragraph counts once
- **WHEN** an added paragraph contains `[[Foo]] and again [[Foo]]`
- **THEN** the count for `[[Foo]]` from this paragraph is 1

#### Scenario: Same link in two paragraphs of one note counts twice
- **WHEN** two separate paragraphs in one note each contain a `[[Foo]]` mention added in the window
- **THEN** the count for `[[Foo]]` is 2

#### Scenario: Same link across multiple notes accumulates
- **WHEN** notes A, B, and C each contribute one paragraph with `[[Foo]]` in the window
- **THEN** the count for `[[Foo]]` is 3

#### Scenario: Markdown link on an added line is counted
- **WHEN** an added paragraph contains `[Foo](foo.md)` resolving to `Foo`
- **THEN** the count for `Foo` from this paragraph is 1 (markdown links count via the unified link kinds)

#### Scenario: Source file deleted after window
- **WHEN** a commit in the window added `[[Foo]]` to a file that has since been deleted
- **THEN** `[[Foo]]` is still counted (via the synthetic-key fallback)
