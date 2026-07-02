## MODIFIED Requirements

### Requirement: Send-to-synth action

The Journal tab SHALL provide a key (`s`) that opens an inline prompt for a target synth note. The prompt SHALL support: (a) fuzzy-picking an existing note marked `ft-synth: true`, or (b) entering a new note name (resolved against `synth.folder`). On confirmation, the tab SHALL call `plan_synth_scaffold` with the currently selected entries (or all displayed entries when no selection), `apply_synth_scaffold` to write the changes, then trigger the existing editor-handoff path opening `$EDITOR` at the bottom of the target file. When sending to an existing note, the scaffold SHALL apply the dedup-on-append invariant: entries whose `(source_path, body)` is already pinned in the picked note SHALL be dropped before planning (handled by `plan_synth_scaffold`'s append path). A second key (`n`) SHALL trigger the "send-to-synth-new-only" flow: after the user picks an existing note, the tab SHALL compute that note's last-synth watermark and ship only entries whose `date` is strictly greater than the watermark's date (in addition to the dedup-on-append invariant); when the watermark is `None`, it SHALL fall back to shipping all missing entries with an informational toast.

#### Scenario: Send selected entries to new synth note

- **WHEN** the user selects two entries, presses `s`, types `topic`, and confirms with the "new note" option
- **THEN** `Synthesis/topic.md` is created with `ft-synth: true` frontmatter and protected sections for the two entries; `$EDITOR` opens at the bottom

#### Scenario: Send to existing synth note dedups

- **WHEN** the user presses `s` and picks an existing `Synthesis/topic.md` that already pins some of the selected entries
- **THEN** only the not-yet-pinned entries are appended and `$EDITOR` opens at the new bottom; existing callouts are unchanged

#### Scenario: No selection sends all displayed entries

- **WHEN** the user presses `s` with no entries selected
- **THEN** all currently displayed entries are sent as the scaffold source (subject to dedup against the picked note)

#### Scenario: Cancel prompt aborts action

- **WHEN** the user presses `s` and then `Esc`
- **THEN** the prompt closes and no file is modified

#### Scenario: send-to-synth-new-only scopes to entries newer than the watermark

- **WHEN** the user presses `n`, picks a synth note whose last callout was pinned at a commit dated 2026-06-01, and confirms
- **THEN** only entries whose `date` is greater than 2026-06-01 are appended (after dedup)

#### Scenario: send-to-synth-new-only on a note with no watermark falls back

- **WHEN** the user presses `n`, picks a synth note with no callouts (or all-unreachable SHAs), and confirms
- **THEN** all missing entries are appended and an informational toast explains the watermark was unavailable

### Requirement: Help overlay covers new bindings

The Journal tab's `Tab::help_sections()` SHALL include entries for: `<space>` (toggle entry selection), `s` (send to synth), `n` (send only entries newer than the picked note's last synth), and `w` (in-window-only toggle, when applicable).

#### Scenario: Help overlay lists new bindings

- **WHEN** the user opens the `?` help overlay on the Journal tab
- **THEN** the overlay lists `space`, `s`, `n`, and `w` with their descriptions
