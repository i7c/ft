## MODIFIED Requirements

### Requirement: ft synth scaffold command

`ft synth <target.md> --link "[[Foo]]" [--link "[[Bar]]" ...] [--since <duration> | --range <X>..<Y>] [--all | --in-window] [--from <path>:<line> ...] [--no-edit]` SHALL generate or append protected-section scaffolding into the target note. `--link` SHALL be repeatable. When the target file does not exist, the command SHALL create it with `ft-synth: true` frontmatter AND, when `--link` is supplied, an `ft-synth-targets` key listing the supplied links, followed by the scaffolded sections as the body. When the target exists, the command SHALL append (at end of file) the new sections separated from existing content by one blank line; the append path SHALL drop any entry whose `(source_path, body)` is already pinned in the note (dedup-on-append invariant), so re-running scaffold with the same target is idempotent. After writing, the command SHALL open `$EDITOR` at the bottom of the file unless `--no-edit` is passed.

#### Scenario: Create new synth note

- **WHEN** `ft synth Synthesis/topic.md --link "[[Foo]]" --since 7d` is run and `Synthesis/topic.md` does not exist
- **THEN** the file is created with `ft-synth: true` and `ft-synth-targets: ["[[Foo]]"]` frontmatter and the scaffolded sections; `$EDITOR` is launched at the bottom of the file

#### Scenario: Append to existing synth note dedups

- **WHEN** `ft synth Synthesis/topic.md --link "[[Bar]]"` is run and the file already exists with some of Bar's paragraphs pinned
- **THEN** only the not-yet-pinned sections are appended (separated by a blank line) and `$EDITOR` is launched at the new bottom; existing content is preserved unchanged

#### Scenario: Re-running scaffold with the same target is idempotent

- **WHEN** `ft synth Synthesis/topic.md --link "[[Foo]]"` is run twice in succession with no source changes
- **THEN** the second run appends zero sections (all entries are already pinned)

#### Scenario: --no-edit suppresses editor handoff

- **WHEN** `ft synth ... --no-edit` is run
- **THEN** the file is written but `$EDITOR` is NOT launched and the command exits 0

#### Scenario: --link is required when no --from given

- **WHEN** neither `--link` nor `--from` is passed and the target does not exist
- **THEN** the command exits with a non-zero code and a clear "one of --link or --from is required" error

## ADDED Requirements

### Requirement: Self-describing synth note targets

A synth note MAY declare its journal target(s) in YAML frontmatter via the key `ft-synth-targets`, whose value SHALL be a YAML sequence of `[[wikilink]]` strings (e.g. `ft-synth-targets: ["[[Foo]]", "[[Bar]]"]`). The key SHALL be optional; notes without it SHALL behave exactly as today (scaffold append, verify, repair, reslice all unchanged). `ft synth scaffold` and `ft synth grow` SHALL write the key when `--link` is supplied and the note is being created, or when appending and the key is absent. The key SHALL NOT affect verify, repair, or reslice. Parsing SHALL be lenient (accept quoted or bare values, `"[[Foo]]"` or `"Foo"`) and SHALL store values as raw wikilink text. A helper `ft_core::synth::callout::parse_synth_targets(content) -> Option<Vec<String>>` SHALL extract the list; a helper `upsert_synth_frontmatter(content, targets: Option<&[String]>)` SHALL idempotently set both `ft-synth: true` and `ft-synth-targets` without clobbering unrelated frontmatter keys.

#### Scenario: Scaffold writes targets on create

- **WHEN** `ft synth scaffold Synthesis/topic.md --link "[[Foo]]" --link "[[Bar]]"` creates a new note
- **THEN** the frontmatter contains `ft-synth: true` and `ft-synth-targets: ["[[Foo]]", "[[Bar]]"]`

#### Scenario: Grow appends targets when key absent

- **WHEN** `ft synth grow Synthesis/topic.md --link "[[Baz]]"` is run on an existing note that lacks `ft-synth-targets`
- **THEN** the frontmatter gains `ft-synth-targets: ["[[Baz]]"]` and existing frontmatter keys are preserved

#### Scenario: Notes without the key are unaffected

- **WHEN** a synth note created before this change (no `ft-synth-targets`) is verified, repaired, or resliced
- **THEN** the commands behave exactly as before the change

#### Scenario: Lenient parsing of hand-authored values

- **WHEN** a note's frontmatter contains `ft-synth-targets: [Foo, "[[Bar]]"]`
- **THEN** `parse_synth_targets` returns `Some(vec!["Foo", "[[Bar]]"])`

#### Scenario: Upsert preserves unrelated frontmatter keys

- **WHEN** `upsert_synth_frontmatter` is applied to a note whose frontmatter has `title: My Note` and `tags: [a]`
- **THEN** the result retains `title` and `tags` unchanged alongside `ft-synth: true` and `ft-synth-targets`
