# paragraph-synth-tui

## MODIFIED Requirements

### Requirement: Commit runs the scaffold plan/apply and editor handoff

On commit, the flow SHALL build a `SynthSource` for each selected paragraph (carrying its effective range and re-sliced body), call `plan_synth_scaffold` then `apply_synth_scaffold`, and hand the resulting file off to `$EDITOR` — identical to the shared send-to-synth commit path used by the Search and Recent tabs. When appending to an existing non-synth note, the flow SHALL first mark the target with the `ft-synth` marker. Sections SHALL append in selection order. Dedup (entries already pinned by `(source_path, body)`) SHALL apply and the count of skipped duplicates SHALL be reported via toast.

#### Scenario: Commit appends protected sections

- **WHEN** two paragraphs are selected and the user commits to an existing synth note
- **THEN** two `[!ft-source]` callouts are appended to the target, each pinning the effective range and body, and `$EDITOR` opens on the target

#### Scenario: Dedup reports already-pinned paragraphs

- **WHEN** a selected paragraph is already pinned byte-identically in the target and the user commits
- **THEN** no new callout is added for it and a toast reports that N entries were skipped as already pinned
