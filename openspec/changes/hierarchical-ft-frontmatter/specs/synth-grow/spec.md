## MODIFIED Requirements

### Requirement: ft notes synth grow command
`ft notes synth grow <note.md> [--link "[[X]]" ...] [--from <path>:<line> ...] [--new-only] [--since <duration> | --range <X>..<Y> [--in-window]] [--limit N] [--no-edit]` SHALL source journal entries, drop those already pinned in the target note, optionally scope to "new since last synth," and append the missing ones via the existing `plan_synth_scaffold` / `apply_synth_scaffold` append path. `grow` SHALL require the target note to already exist (use `ft notes synth scaffold` to create one); running `grow` on a non-existent note SHALL exit non-zero with a clear message.

When `--link` (or `--from`) is supplied, those targets SHALL be used. When neither `--link` nor `--from` is supplied, `grow` SHALL read targets from the note's nested `ft.synth.targets` frontmatter; if that key is absent, `grow` SHALL exit non-zero with a message telling the user to pass `--link` or add `ft.synth.targets` frontmatter. The legacy flat `ft-synth-targets` key SHALL NOT be recognized. Explicit `--link` SHALL override frontmatter targets.

`--new-only` SHALL compute the note's last-synth watermark and keep only entries whose `date` is strictly greater than the watermark's date. When the watermark is `None` (no callouts or all SHAs unreachable), `--new-only` SHALL fall back to "all missing" with a warning naming the reason. `--limit N` SHALL cap the number of appended sections to the newest `N` after dedup and (if active) the new-only filter; the journal's date-descending order SHALL be preserved.

`--since`/`--range`/`--in-window` SHALL have the same semantics as `ft notes synth scaffold`. After writing, `grow` SHALL open `$EDITOR` unless `--no-edit` is passed.

#### Scenario: Grow appends only missing entries

- **WHEN** `ft notes synth grow Synthesis/topic.md --link "[[Foo]]"` is run on a note that already pins half of Foo's journal entries
- **THEN** only the not-yet-pinned entries are appended; existing callouts are unchanged

#### Scenario: Grow with --new-only scopes to entries newer than the last synth

- **WHEN** `ft notes synth grow Synthesis/topic.md --link "[[Foo]]" --new-only` is run and the note's last callout was pinned at commit `B` dated 2026-06-01
- **THEN** only journal entries whose `date` is greater than 2026-06-01 are appended

#### Scenario: Grow --new-only on a brand-new note falls back to all missing

- **WHEN** `ft notes synth grow Synthesis/topic.md --link "[[Foo]]" --new-only` is run on a note with no callouts
- **THEN** all of Foo's journal entries are appended and a warning is printed explaining the watermark was unavailable

#### Scenario: Grow reads targets from nested frontmatter

- **WHEN** `ft notes synth grow Synthesis/topic.md --new-only` is run on a note whose frontmatter contains `ft:\n  synth:\n    targets: ["[[Foo]]"]` and no `--link` is passed
- **THEN** the journal is built for `[[Foo]]` and only missing, newer-than-watermark entries are appended

#### Scenario: Grow with no targets errors clearly

- **WHEN** `ft notes synth grow Synthesis/topic.md --new-only` is run on a note with no nested `ft.synth.targets` frontmatter and no `--link`/`--from`
- **THEN** the command exits non-zero with a message directing the user to pass `--link` or add `ft.synth.targets` frontmatter

#### Scenario: Grow on a non-existent note errors

- **WHEN** `ft notes synth grow Synthesis/missing.md --link "[[Foo]]"` is run and `Synthesis/missing.md` does not exist
- **THEN** the command exits non-zero with a message directing the user to `ft notes synth scaffold`

#### Scenario: Grow --limit caps appended sections

- **WHEN** `ft notes synth grow Synthesis/topic.md --link "[[Foo]]" --limit 5` is run and 12 missing entries exist
- **THEN** exactly 5 sections are appended, the 5 newest by journal date

#### Scenario: Grow honors --no-edit

- **WHEN** `ft notes synth grow ... --no-edit` is run
- **THEN** the file is written but `$EDITOR` is NOT launched

#### Scenario: Grow reports already-pinned count

- **WHEN** `ft notes synth grow Synthesis/topic.md --link "[[Foo]]"` is run and 3 of 8 entries are already pinned
- **THEN** the command prints a summary line reporting that 3 were already pinned and 5 were appended
