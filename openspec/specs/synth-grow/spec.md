# synth-grow Specification

## Purpose
TBD - created by archiving change synth-grow-accrete. Update Purpose after archive.
## Requirements
### Requirement: Last-synth watermark

A pure helper `ft_core::synth::accrete::last_synth_watermark(repo_root, existing_callouts) -> Result<Option<(git2::Oid, NaiveDate)>>` SHALL compute the last-synth watermark from a synth note's existing `[!ft-source]` callouts. The watermark is the topological tip among the callouts' pinned `commit_sha` values (the descendant reachable from all of them), paired with that commit's committer date. Each pinned SHA SHALL be verified reachable via `git cat-file -e` before inclusion; unreachable SHAs SHALL be skipped. When all SHAs are unreachable, or the callout list is empty, the result SHALL be `Ok(None)`. The 7-hex short SHAs stored in callouts SHALL be resolved by git; an ambiguous short SHA SHALL surface as an `Error::SynthWatermark` diagnostic naming the offending SHA.

#### Scenario: Descendant tip among multiple pins

- **WHEN** a synth note has callouts pinned to SHAs `A` (older) and `B` (newer, a descendant of `A`)
- **THEN** `last_synth_watermark` returns `Some((B, <date of B>))`

#### Scenario: Brand-new note has no watermark

- **WHEN** a synth note has zero callouts
- **THEN** `last_synth_watermark` returns `Ok(None)`

#### Scenario: Unreachable SHA is skipped

- **WHEN** a synth note has two callouts, one pinned to a SHA unreachable in the local history and one pinned to a reachable SHA
- **THEN** `last_synth_watermark` skips the unreachable SHA and returns the tip among the reachable ones

#### Scenario: All SHAs unreachable degrades to None

- **WHEN** every callout's pinned SHA is unreachable and the note has at least one callout
- **THEN** `last_synth_watermark` returns `Ok(None)`

#### Scenario: Ambiguous short SHA surfaces an error

- **WHEN** a callout's 7-hex SHA prefix matches more than one commit in the repository
- **THEN** `last_synth_watermark` returns `Err(Error::SynthWatermark)` naming the ambiguous SHA

### Requirement: Missing-entry filter

A pure helper `ft_core::synth::accrete::filter_missing(existing_callouts, entries) -> Vec<JournalEntry>` SHALL drop any entry whose body text is already pinned in `existing_callouts`. The dedup key SHALL be the pair `(source_path, body)` where `source_path` is the entry's vault-relative source path and `body` is the entry's `section_text` compared byte-for-byte against a callout's unprefixed body. The 6-hex `content_hash` MAY be used as a fast pre-filter but the body comparison SHALL be exact. The `commit_sha` of an existing callout SHALL NOT be part of the key. Entry order from the input SHALL be preserved among the surviving entries.

#### Scenario: Unchanged paragraph is dropped

- **WHEN** a journal entry's `(source_path, body)` exactly matches an existing callout in the note
- **THEN** the entry is dropped from the result

#### Scenario: Updated paragraph is kept

- **WHEN** a journal entry's `source_path` matches an existing callout's path but its `body` differs (the paragraph was edited since it was pinned)
- **THEN** the entry is kept in the result

#### Scenario: Brand-new paragraph is kept

- **WHEN** a journal entry's `source_path` does not match any existing callout's path
- **THEN** the entry is kept in the result

#### Scenario: Order preserved among survivors

- **WHEN** `filter_missing` is given entries `[A, B, C]` and `B` is already pinned
- **THEN** the result is `[A, C]` preserving input order

### Requirement: ft synth grow command

`ft synth grow <note.md> [--link "[[X]]" ...] [--from <path>:<line> ...] [--new-only] [--since <duration> | --range <X>..<Y> [--in-window]] [--limit N] [--no-edit]` SHALL source journal entries, drop those already pinned in the target note, optionally scope to "new since last synth," and append the missing ones via the existing `plan_synth_scaffold` / `apply_synth_scaffold` append path. `grow` SHALL require the target note to already exist (use `ft synth scaffold` to create one); running `grow` on a non-existent note SHALL exit non-zero with a clear message.

When `--link` (or `--from`) is supplied, those targets SHALL be used. When neither `--link` nor `--from` is supplied, `grow` SHALL read targets from the note's `ft-synth-targets` frontmatter; if that key is absent, `grow` SHALL exit non-zero with a message telling the user to pass `--link` or add `ft-synth-targets` frontmatter. Explicit `--link` SHALL override frontmatter targets.

`--new-only` SHALL compute the note's last-synth watermark and keep only entries whose `date` is strictly greater than the watermark's date. When the watermark is `None` (no callouts or all SHAs unreachable), `--new-only` SHALL fall back to "all missing" with a warning naming the reason. `--limit N` SHALL cap the number of appended sections to the newest `N` after dedup and (if active) the new-only filter; the journal's date-descending order SHALL be preserved.

`--since`/`--range`/`--in-window` SHALL have the same semantics as `ft synth scaffold`. After writing, `grow` SHALL open `$EDITOR` unless `--no-edit` is passed.

#### Scenario: Grow appends only missing entries

- **WHEN** `ft synth grow Synthesis/topic.md --link "[[Foo]]"` is run on a note that already pins half of Foo's journal entries
- **THEN** only the not-yet-pinned entries are appended; existing callouts are unchanged

#### Scenario: Grow with --new-only scopes to entries newer than the last synth

- **WHEN** `ft synth grow Synthesis/topic.md --link "[[Foo]]" --new-only` is run and the note's last callout was pinned at commit `B` dated 2026-06-01
- **THEN** only journal entries whose `date` is greater than 2026-06-01 are appended

#### Scenario: Grow --new-only on a brand-new note falls back to all missing

- **WHEN** `ft synth grow Synthesis/topic.md --link "[[Foo]]" --new-only` is run on a note with no callouts
- **THEN** all of Foo's journal entries are appended and a warning is printed explaining the watermark was unavailable

#### Scenario: Grow reads targets from frontmatter

- **WHEN** `ft synth grow Synthesis/topic.md --new-only` is run on a note whose frontmatter contains `ft-synth-targets: ["[[Foo]]"]` and no `--link` is passed
- **THEN** the journal is built for `[[Foo]]` and only missing, newer-than-watermark entries are appended

#### Scenario: Grow with no targets errors clearly

- **WHEN** `ft synth grow Synthesis/topic.md --new-only` is run on a note with no `ft-synth-targets` frontmatter and no `--link`/`--from`
- **THEN** the command exits non-zero with "pass --link or add ft-synth-targets frontmatter"

#### Scenario: Grow on a non-existent note errors

- **WHEN** `ft synth grow Synthesis/missing.md --link "[[Foo]]"` is run and `Synthesis/missing.md` does not exist
- **THEN** the command exits non-zero with a message directing the user to `ft synth scaffold`

#### Scenario: Grow --limit caps appended sections

- **WHEN** `ft synth grow Synthesis/topic.md --link "[[Foo]]" --limit 5` is run and 12 missing entries exist
- **THEN** exactly 5 sections are appended, the 5 newest by journal date

#### Scenario: Grow honors --no-edit

- **WHEN** `ft synth grow ... --no-edit` is run
- **THEN** the file is written but `$EDITOR` is NOT launched

#### Scenario: Grow reports already-pinned count

- **WHEN** `ft synth grow Synthesis/topic.md --link "[[Foo]]"` is run and 3 of 8 journal entries were already pinned
- **THEN** the output reports "appended 5 section(s) (3 already pinned, skipped)"

