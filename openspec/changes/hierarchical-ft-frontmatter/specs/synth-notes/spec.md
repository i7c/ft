## MODIFIED Requirements

### Requirement: Synth note frontmatter marker
A synth note SHALL be identified by the presence of `ft.synth.enabled: true` in the nested `ft:` YAML frontmatter map. The legacy flat `ft-synth: true` key SHALL NOT be recognized — a note carrying only the legacy marker SHALL NOT be treated as a synth note. Notes without the nested marker SHALL NOT be treated as synth notes by any `ft` feature. `ft.synth.enabled: false` (or absence of the key) SHALL mean the note is not a synth note. The marker SHALL be respected regardless of where the note lives in the vault; the `synth.folder` config is convenience for the scaffold-create path, not enforcement.

#### Scenario: Nested marker identifies synth note

- **WHEN** `Synthesis/topic.md` starts with `---\nft:\n  synth:\n    enabled: true\n---\n`
- **THEN** `ft notes synth verify --all` includes it; the link-review treats its `[!ft-source]` callouts as protected sections

#### Scenario: Legacy flat marker is not recognized

- **WHEN** `Synthesis/topic.md` starts with `---\nft-synth: true\n---\n` (legacy flat form) and has no nested `ft:` map
- **THEN** the note is NOT treated as a synth note (`is_synth_note` returns `false`); `ft notes synth verify --all` does not sweep it

#### Scenario: Note without marker is not synth

- **WHEN** a note in `Synthesis/` lacks the nested `ft.synth.enabled: true` marker
- **THEN** it is treated as a regular note (callouts inside it do not protect anything from link-review counting)

### Requirement: Self-describing synth note targets

A synth note MAY declare its journal target(s) in YAML frontmatter via the nested key `ft.synth.targets`, whose value SHALL be a YAML sequence of `[[wikilink]]` strings (e.g. `ft:\n  synth:\n    targets: ["[[Foo]]", "[[Bar]]"]`). The legacy flat key `ft-synth-targets` SHALL NOT be recognized. The key SHALL be optional; notes without it SHALL behave exactly as today (scaffold append, verify, repair, reslice all unchanged). `ft notes synth scaffold` and `ft notes synth grow` SHALL write the nested `ft.synth.targets` key when `--link` is supplied and the note is being created, or when appending and the key is absent. The key SHALL NOT affect verify, repair, or reslice. Parsing SHALL be lenient (accept quoted or bare values, `"[[Foo]]"` or `"Foo"`) and SHALL store values as raw wikilink text. A helper `ft_core::synth::callout::parse_synth_targets(content) -> Option<Vec<String>>` SHALL extract the nested list; a helper `upsert_synth_frontmatter(content, targets: Option<&[String]>)` SHALL idempotently set `ft.synth.enabled: true` and `ft.synth.targets` (nested form only), SHALL remove any legacy `ft-synth:` / `ft-synth-targets:` lines it encounters, and SHALL preserve unrelated frontmatter keys.

#### Scenario: Scaffold writes targets on create (nested form)

- **WHEN** `ft notes synth scaffold Synthesis/topic.md --link "[[Foo]]" --link "[[Bar]]"` creates a new note
- **THEN** the frontmatter contains `ft:\n  synth:\n    enabled: true\n    targets: ["[[Foo]]", "[[Bar]]"]` and does NOT contain the legacy flat `ft-synth` or `ft-synth-targets` keys

#### Scenario: Grow reads targets from nested frontmatter

- **WHEN** `ft notes synth grow Synthesis/topic.md --new-only` is run on a note whose frontmatter contains `ft:\n  synth:\n    targets: ["[[Foo]]"]` and no `--link` is passed
- **THEN** the journal is built for `[[Foo]]` and only missing, newer-than-watermark entries are appended

#### Scenario: Grow appends targets when key absent

- **WHEN** `ft notes synth grow Synthesis/topic.md --link "[[Baz]]"` is run on an existing note that lacks nested `ft.synth.targets`
- **THEN** the frontmatter gains the nested `ft.synth.targets: ["[[Baz]]"]` and existing frontmatter keys are preserved

#### Scenario: Notes without the key are unaffected

- **WHEN** a synth note with no `ft.synth.targets` key is verified, repaired, or resliced
- **THEN** the commands behave exactly as before the change

#### Scenario: Lenient parsing of hand-authored values

- **WHEN** a note's frontmatter contains `ft:\n  synth:\n    targets: [Foo, "[[Bar]]"]`
- **THEN** `parse_synth_targets` returns `Some(vec!["Foo", "[[Bar]]"])`

#### Scenario: Legacy flat targets key is not recognized

- **WHEN** a note's frontmatter contains the legacy `ft-synth-targets: ["[[Foo]]"]` and no nested `ft:` map
- **THEN** `parse_synth_targets` SHALL return `None` (the legacy key is ignored)

#### Scenario: Upsert preserves unrelated frontmatter keys and strips legacy

- **WHEN** `upsert_synth_frontmatter` is applied to a note whose frontmatter has `title: My Note`, `tags: [a]`, and legacy `ft-synth: true`
- **THEN** the result retains `title` and `tags` unchanged alongside the nested `ft.synth.enabled: true` (and targets, if supplied), and SHALL NOT contain the legacy `ft-synth:` line
