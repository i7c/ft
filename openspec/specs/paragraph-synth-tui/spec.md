# paragraph-synth-tui Specification

## Purpose
Source-driven copy-to-synth TUI flow: pick a note (Graph tab `y` on a Note node, or Notes tab `y` → source picker), multi-select its paragraphs with shrink-only range adjust, and pin them into a target note as protected `[!ft-source]` callouts via the existing scaffold plan/apply path. The source-driven sibling of the gather/recent feed-driven send-to-synth flows.

## Requirements
### Requirement: Paragraph-synth entry point on the Graph tab
The TUI SHALL provide a `graph.synth-from-note` command bound to `y` on the Graph tab. When a Note node is focused and `y` is pressed, the TUI SHALL first check the git working-tree status; if the tree is not clean, the TUI SHALL emit an error toast and NOT open the paragraph-synth modal. When the tree is clean, the TUI SHALL open the paragraph-synth modal seeded directly into the paragraph-multi-select step with the focused note as the source (no source-picker step, no confirmation step). Ghost, Heading, Paragraph, Task, and Directory nodes SHALL NOT trigger the flow; pressing `y` on a non-Note node SHALL be a no-op (or fall through to the tab's other bindings).

#### Scenario: Clean tree, Note focused
- **WHEN** the Graph tab has a Note node focused, the git working tree is clean, and `y` is pressed
- **THEN** the paragraph-synth modal opens at the paragraph-multi-select step with the focused note as the source

#### Scenario: Dirty tree aborts before opening the modal
- **WHEN** the git working tree is not clean (modified, untracked, deleted, or conflicted files exist) and `y` is pressed on a focused Note node
- **THEN** the TUI emits an error toast mentioning the dirty working tree and does NOT open the paragraph-synth modal

#### Scenario: Non-Note node is a no-op
- **WHEN** a Heading, Paragraph, Task, Ghost, or Directory node is focused and `y` is pressed
- **THEN** the paragraph-synth modal does not open

### Requirement: Paragraph-synth entry point on the Notes tab
The TUI SHALL provide a `notes.synth-from-note` command bound to `y` on the Notes tab. Pressing `y` SHALL first check the git working-tree status; if the tree is not clean, the TUI SHALL emit an error toast and NOT open the paragraph-synth modal. When the tree is clean, the TUI SHALL open the paragraph-synth modal at the source-note-picker step (a fuzzy picker over vault note files). Selecting a note SHALL advance the modal to the paragraph-multi-select step seeded to that note. Cancelling the picker SHALL close the modal with no state change.

#### Scenario: Clean tree opens the source picker
- **WHEN** the Notes tab is active, the git working tree is clean, and `y` is pressed
- **THEN** the paragraph-synth modal opens at the source-note-picker step

#### Scenario: Dirty tree aborts on the Notes tab
- **WHEN** the git working tree is not clean and `y` is pressed on the Notes tab
- **THEN** the TUI emits an error toast mentioning the dirty working tree and does NOT open the paragraph-synth modal

#### Scenario: Selecting a source note advances to paragraph select
- **WHEN** a note is selected in the source-note picker
- **THEN** the modal advances to the paragraph-multi-select step seeded to that note's content

### Requirement: Paragraph multi-select step
The paragraph-synth modal's paragraph-multi-select step SHALL list every paragraph of the source note (via `extract_paragraphs`) in document order in a top list pane, and render a preview of the focused paragraph in a bottom pane (reusing the shared list/preview split widget). Each list row SHALL show the paragraph's line range. `j`/`k` (and arrow keys) SHALL move the focus; `Space` SHALL toggle the focused paragraph's selection; `Enter` SHALL advance to the target-pick step carrying the selected paragraphs (and SHALL emit an error toast and stay if no paragraph is selected). `Esc` SHALL return to the previous step (source picker on the Notes tab; close the modal on the Graph tab).

#### Scenario: Paragraphs listed in document order
- **WHEN** the source note contains three paragraphs at L1-3, L5-7, L9-12
- **THEN** the list pane shows three rows in that order, each with its line range

#### Scenario: Toggle and advance
- **WHEN** the user presses `Space` on a paragraph and then `Enter`
- **THEN** the modal advances to the target-pick step carrying exactly the toggled paragraph

#### Scenario: Enter with no selection is rejected
- **WHEN** the user presses `Enter` with no paragraph selected
- **THEN** the modal emits an error toast ("select at least one paragraph") and stays at the paragraph-multi-select step

### Requirement: Shrink-only range adjustment for the focused paragraph
The paragraph-multi-select step SHALL support shrinking the line range of the focused paragraph. `[` SHALL trim one line from the top of the effective range; `]` SHALL trim one line from the bottom; `r` SHALL reset the focused paragraph to its full original range. The effective range SHALL be `(line_start + top_trim) ..= (line_end - bot_trim)`. The adjustment SHALL always leave at least one line in the effective range; attempts to shrink past this floor SHALL be clamped (no-op). The preview pane SHALL reflect the effective range: lines within it SHALL be visually distinguishable from lines trimmed away. Range adjustment SHALL operate on the focused paragraph regardless of whether it is selected.

#### Scenario: Shrink top with `[`
- **WHEN** the focused paragraph spans L12-18 and the user presses `[`
- **THEN** the effective range becomes L13-18 and the preview shows L12 as trimmed and L13-18 as the effective range

#### Scenario: Shrink bottom with `]`
- **WHEN** the focused paragraph spans L12-18 and the user presses `]`
- **THEN** the effective range becomes L12-17

#### Scenario: Reset to full paragraph with `r`
- **WHEN** the focused paragraph has been shrunk to L14-18 from an original L12-18 and the user presses `r`
- **THEN** the effective range resets to L12-18

#### Scenario: Floor of one line enforced
- **WHEN** the focused paragraph spans a single line L5-5 and the user presses `[` or `]`
- **THEN** the effective range remains L5-5 (no-op, clamped)

#### Scenario: Adjust visible in preview
- **WHEN** the focused paragraph has been shrunk
- **THEN** the preview pane distinguishes lines within the effective range from trimmed lines

### Requirement: Pinned section captures the adjusted range
When the flow commits, each pinned `[!ft-source]` callout SHALL capture the paragraph's *effective* (post-adjustment) line range and a body re-sliced from the source content at that effective range — NOT the original full-paragraph range/text. The content hash SHALL be computed over the re-sliced body. A paragraph that was not adjusted SHALL pin its full original range and text.

#### Scenario: Adjusted paragraph pins the trimmed range
- **WHEN** a paragraph originally at L12-18 was shrunk to L14-18 and the flow commits
- **THEN** the resulting `[!ft-source]` callout header shows `L14-18` and the body is the lines L14-18 of the source, and the content hash is blake3 of that body

#### Scenario: Unadjusted paragraph pins the full range
- **WHEN** a paragraph at L20-24 was selected without any adjustment and the flow commits
- **THEN** the resulting callout header shows `L20-24` and the body is the full paragraph text

### Requirement: Target pick reuses the send-to-synth split
The paragraph-synth target-pick step SHALL offer two paths: pick an existing note (`s`), or create a new note (`S`, folder → title → template + variable prompts). The step SHALL preserve the paragraph-multi-select state so `Esc` returns to it with the prior selections and adjustments intact. Selecting the same file as the source SHALL be rejected inline with an error shown in the modal footer (the source cannot also be the synth target).

#### Scenario: Pick existing note advances to commit
- **WHEN** the user selects an existing note in the target picker
- **THEN** the flow proceeds to commit against that target

#### Scenario: Create new note via template
- **WHEN** the user presses `S` and completes the folder → title → template → variable prompts
- **THEN** the flow commits against the newly created target

#### Scenario: Esc preserves multi-select state
- **WHEN** the user presses `Esc` at the target-pick step
- **THEN** the modal returns to the paragraph-multi-select step with the previously selected paragraphs and their adjustments intact

#### Scenario: Source-equals-target rejected
- **WHEN** the user selects the source note itself as the target
- **THEN** an error is shown in the modal footer and the step stays at target pick

### Requirement: Commit runs the scaffold plan/apply and editor handoff
On commit, the flow SHALL build a `SynthSource` for each selected paragraph (carrying its effective range and re-sliced body), call `plan_synth_scaffold` then `apply_synth_scaffold`, and hand the resulting file off to `$EDITOR` — identical to the gather tab's send-to-synth commit path. When appending to an existing non-synth note, the flow SHALL first mark the target with the `ft-synth` marker. Sections SHALL append in selection order. Dedup (entries already pinned by `(source_path, body)`) SHALL apply and the count of skipped duplicates SHALL be reported via toast.

#### Scenario: Commit appends protected sections
- **WHEN** two paragraphs are selected and the user commits to an existing synth note
- **THEN** two `[!ft-source]` callouts are appended to the target, each pinning the effective range and body, and `$EDITOR` opens on the target

#### Scenario: Dedup reports already-pinned paragraphs
- **WHEN** a selected paragraph is already pinned byte-identically in the target and the user commits
- **THEN** no new callout is added for it and a toast reports that N entries were skipped as already pinned

#### Scenario: New target is marked synth
- **WHEN** the target is a newly created note
- **THEN** the note's frontmatter includes the synth marker before the scaffold is applied

### Requirement: Dirty-source detection still applies at commit
The existing `plan_synth_scaffold` dirty/untracked-source refusal SHALL remain in effect at commit time. If a source file has become dirty or untracked between the entry guard and commit, the commit SHALL fail with the `SynthDirtySources` error surfaced as a toast, and the modal SHALL return to the paragraph-multi-select step (state preserved) so the user can retry after committing/stashing.

#### Scenario: Source dirtied mid-flow fails at commit
- **WHEN** the source file is clean at entry but becomes dirty before commit
- **THEN** the commit fails with a dirty-sources error toast and the modal returns to the paragraph-multi-select step
