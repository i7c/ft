## MODIFIED Requirements

### Requirement: score_related function
`ft_core::related::score_related(graph: &Graph, note_id: NoteId, vault: &Vault) -> Result<Vec<RelatedScore>>` SHALL compute a co-occurrence score for every concept (note or ghost) that appears in the graph alongside the target N. N SHALL be either a `NodeKind::Note` or a `NodeKind::Ghost`. `RelatedScore` SHALL carry: `note_id: NoteId`, `title: String`, `score: u32`, `already_in_related: bool`.

Scoring rules:
- **+3** for each `NodeKind::Paragraph` that has `ParagraphLink` edges to both N (or any alias) and C
- **+1** for each vault file where at least one paragraph links to N and at least one *different* paragraph links to C (same-file cross-paragraph co-occurrence)

N itself and N's aliases SHALL be excluded from the scored results. Concepts scoring 0 SHALL be omitted.

**Ghost targets:** when N is a `NodeKind::Ghost`, alias resolution SHALL be skipped (a ghost has no Related section and no backing file to read), so the alias set is empty and `already_in_related` SHALL be `false` for every returned row. The co-occurrence walk SHALL run unchanged against the ghost, since ghosts can be the target of incoming `ParagraphLink` edges. This mirrors `ft_core::journal::build_journal`'s ghost handling (`NodeKind::Ghost(_) => note_path = None, aliases = []`).

#### Scenario: Same-paragraph co-occurrence scores 3
- **WHEN** a paragraph node has ParagraphLink edges to both N and concept C
- **THEN** C receives +3 in its score

#### Scenario: Same-file cross-paragraph co-occurrence scores 1
- **WHEN** file F has paragraph P1 linking to N and paragraph P2 (different from P1) linking to C, with P2 not linking to N
- **THEN** C receives +1 from file F

#### Scenario: Same paragraph counts only once per paragraph
- **WHEN** a paragraph has two ParagraphLink edges to C alongside one to N
- **THEN** C still receives only +3 (not +6) for that paragraph

#### Scenario: N excluded from results
- **WHEN** `score_related` is called for note N
- **THEN** N does not appear in the returned `Vec<RelatedScore>`

#### Scenario: Zero-score concepts omitted
- **WHEN** a concept C appears in the vault but never in a paragraph or file that also contains N
- **THEN** C is not present in the returned results

#### Scenario: Ghost target produces scored concepts
- **WHEN** `score_related` is called for a `NodeKind::Ghost` N, and paragraphs link to both N and concept C
- **THEN** C appears in the results with its co-occurrence score, and `already_in_related` is `false` for every returned row

#### Scenario: Ghost target skips alias resolution
- **WHEN** `score_related` is called for a `NodeKind::Ghost` N
- **THEN** no alias set is read (there is no Related section to consult), and no returned row has `already_in_related == true`

### Requirement: TUI graph-tab Related updater modal
The graph tab SHALL support a Related modal overlay triggered when a `NodeKind::Note` node is selected and the user presses `R` (displayed in help as `Shift+R`, normalizing to the same chord form as `Shift+J` for the Journal tab). The modal is a unified **read + write** surface: it reads the scored concept list (the same data `ft notes related` prints) and optionally writes via commit. The modal SHALL display:

- A header identifying the note, titled `Related: <note title>` (the modal is no longer framed as write-only; "Update" wording is dropped)
- A scrollable list of `RelatedScore` entries sorted by: already-in-related first (marked, non-interactive), then candidate concepts sorted descending by score
- Checkboxes (Space to toggle) on candidate concepts
- A confirm action (Enter) that calls `apply_related_update` for all checked entries and closes the modal
- A cancel action (Esc / `q`) that closes the modal without writing — the modal is safe to open purely for reading and close without committing

The modal SHALL remain Note-only: it SHALL NOT open for `NodeKind::Ghost` rows (a ghost has no file to write to). Ghost reading is delivered by the `ft notes related` print command. Selecting a ghost row and pressing `R` SHALL surface a toast indicating a note row is required.

#### Scenario: Modal shows existing Related entries as marked
- **WHEN** the modal opens for note N whose Related section already contains `[[Foo]]`
- **THEN** Foo appears at the top of the list marked as already added, with no checkbox

#### Scenario: Toggle and confirm appends entries
- **WHEN** the user checks `[[Bar]]` and `[[Baz]]` and presses Enter
- **THEN** `## Related` in note N gains `- [[Bar]]\n- [[Baz]]\n` appended, and the modal closes

#### Scenario: Cancel discards selection
- **WHEN** the user checks entries and then presses Escape
- **THEN** no changes are written to the note file

#### Scenario: Modal keybinding appears in help overlay
- **WHEN** the user presses `?` on the graph tab
- **THEN** the help overlay lists the Related keybinding (`Shift+R`) under the graph tab's keymap section, with wording reflecting the unified read/write panel (not "updater")

#### Scenario: Modal is read-safe without committing
- **WHEN** the user opens the modal, browses the scored concepts, and presses Esc without checking any entry
- **THEN** the modal closes and no file is written (the modal serves as a reading surface)

#### Scenario: Ghost row does not open the modal
- **WHEN** a `NodeKind::Ghost` row is selected and the user presses `R`
- **THEN** the modal does not open and a toast indicates a note row is required (ghost reading is via `ft notes related`)
