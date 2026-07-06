# synth-notes Specification

## Purpose
TBD - created by archiving change add-synthesis-flow. Update Purpose after archive.
## Requirements
### Requirement: Synth note frontmatter marker
A synth note SHALL be identified by the presence of `ft-synth: true` in its YAML frontmatter. Notes without this marker SHALL NOT be treated as synth notes by any `ft` feature. The marker SHALL be respected regardless of where the note lives in the vault; the `synth.folder` config is convenience for the scaffold-create path, not enforcement.

#### Scenario: Marker identifies synth note
- **WHEN** `Synthesis/topic.md` starts with `---\nft-synth: true\n---\n`
- **THEN** `ft notes synth verify --all` includes it; the link-review treats its `[!ft-source]` callouts as protected sections

#### Scenario: Note without marker is not synth
- **WHEN** a note in `Synthesis/` lacks the `ft-synth: true` marker
- **THEN** it is treated as a regular note (callouts inside it do not protect anything from link-review counting)

### Requirement: Protected-section callout grammar
A protected section SHALL be an Obsidian-style callout whose header line matches the form `> [!ft-source] "<vault-rel-path>" L<a>-<b> @<sha7> #<hash6>` and whose body consists of one or more `> <line>` lines. The vault-relative path SHALL be wrapped in straight double-quotes (`"`) so the grammar remains unambiguous regardless of whether the path contains spaces. Paths containing a literal `"` character are not supported. Each protected section SHALL correspond to exactly one source paragraph (no fusing of adjacent paragraphs from the same source). The header tokens SHALL appear in fixed order; whitespace between tokens SHALL be one or more space characters. The hash SHALL be the first 6 hex chars of the blake3 digest of the source paragraph text (joined with `\n`, no trailing newline). The commit hash SHALL be the first 7 hex chars of the full git commit SHA.

#### Scenario: Well-formed protected section
- **WHEN** a synth note contains:
  ```
  > [!ft-source] "notes/foo.md" L42-44 @abc1234 #7f3a91
  > Some original paragraph
  > spanning two lines.
  ```
- **THEN** `ft notes synth verify` recognizes and validates this section

#### Scenario: One callout per source paragraph
- **WHEN** the scaffold generator pulls two adjacent paragraphs (lines 42-44 and 46-48) from `notes/foo.md`
- **THEN** the scaffold writes two separate callouts with separate headers, not one fused callout

#### Scenario: Malformed header detected
- **WHEN** a callout header is missing a token (e.g. no `#<hash>`)
- **THEN** `ft notes synth verify` reports the section as `malformed` with the path:line of the broken header

### Requirement: Protected-section verification
`ft notes synth verify <note.md>` SHALL parse every `[!ft-source]` callout in the note and, for each, fetch the source git blob at `(commit, vault-rel-path)`, slice lines `a..=b` (1-indexed inclusive), strip the `> ` prefix from each body line in the callout, and compare the two texts joined by `\n` for byte-equality. It SHALL also re-compute blake3 of the body text and confirm the first 6 hex chars equal the header's `#hash6`. Both comparisons MUST succeed for the section to be reported `ok`.

#### Scenario: Section content matches source
- **WHEN** the body text matches the git blob slice byte-for-byte and the content hash matches
- **THEN** the section is reported `ok`

#### Scenario: Body edited in synth note (content drift)
- **WHEN** the body text in the callout differs from the git blob slice
- **THEN** the section is reported `drifted` with the path:line of the callout header

#### Scenario: Source path no longer present at commit
- **WHEN** the path at the pinned commit cannot be resolved (e.g., file renamed before commit was made)
- **THEN** the section is reported `source-missing` with the path:line

#### Scenario: Pinned commit unreachable
- **WHEN** the pinned commit hash is not present in the local git history
- **THEN** the section is reported `source-missing` with a diagnostic naming the commit

### Requirement: ft notes synth verify command
`ft notes synth verify` SHALL accept either a single `<note.md>` path argument or `--all`. With `--all`, the command SHALL walk every `.md` file in the vault, identify those with `ft-synth: true` frontmatter, and run verification across all of them. The exit code SHALL be 0 if every section reports `ok`, 1 otherwise.

#### Scenario: Verify single note all ok
- **WHEN** `ft notes synth verify Synthesis/topic.md` is run on a note whose sections all pass
- **THEN** the command exits 0 and prints one line per section: `ok | <header info>`

#### Scenario: Verify with drift
- **WHEN** any section reports `drifted`, `source-missing`, or `malformed`
- **THEN** the command exits 1 and prints one line per section with the status and location

#### Scenario: --all sweeps the vault
- **WHEN** `ft notes synth verify --all` is run in a vault with three synth notes
- **THEN** all three notes are verified and the report groups by note

#### Scenario: JSON output
- **WHEN** `ft notes synth verify --all --json` is run
- **THEN** stdout is a JSON array where each element has `note` (path), `header` (raw header line), `status` (`ok`/`drifted`/`source-missing`/`malformed`), and `detail` (string)

### Requirement: ft notes synth scaffold command

`ft notes synth <target.md> --link "[[Foo]]" [--link "[[Bar]]" ...] [--since <duration> | --range <X>..<Y>] [--all | --in-window] [--from <path>:<line> ...] [--no-edit]` SHALL generate or append protected-section scaffolding into the target note. `--link` SHALL be repeatable. When the target file does not exist, the command SHALL create it with `ft-synth: true` frontmatter AND, when `--link` is supplied, an `ft-synth-targets` key listing the supplied links, followed by the scaffolded sections as the body. When the target exists, the command SHALL append (at end of file) the new sections separated from existing content by one blank line; the append path SHALL drop any entry whose `(source_path, body)` is already pinned in the note (dedup-on-append invariant), so re-running scaffold with the same target is idempotent. After writing, the command SHALL open `$EDITOR` at the bottom of the file unless `--no-edit` is passed.

#### Scenario: Create new synth note

- **WHEN** `ft notes synth Synthesis/topic.md --link "[[Foo]]" --since 7d` is run and `Synthesis/topic.md` does not exist
- **THEN** the file is created with `ft-synth: true` and `ft-synth-targets: ["[[Foo]]"]` frontmatter and the scaffolded sections; `$EDITOR` is launched at the bottom of the file

#### Scenario: Append to existing synth note dedups

- **WHEN** `ft notes synth Synthesis/topic.md --link "[[Bar]]"` is run and the file already exists with some of Bar's paragraphs pinned
- **THEN** only the not-yet-pinned sections are appended (separated by a blank line) and `$EDITOR` is launched at the new bottom; existing content is preserved unchanged

#### Scenario: Re-running scaffold with the same target is idempotent

- **WHEN** `ft notes synth Synthesis/topic.md --link "[[Foo]]"` is run twice in succession with no source changes
- **THEN** the second run appends zero sections (all entries are already pinned)

#### Scenario: --no-edit suppresses editor handoff

- **WHEN** `ft notes synth ... --no-edit` is run
- **THEN** the file is written but `$EDITOR` is NOT launched and the command exits 0

#### Scenario: --link is required when no --from given

- **WHEN** neither `--link` nor `--from` is passed and the target does not exist
- **THEN** the command exits with a non-zero code and a clear "one of --link or --from is required" error

### Requirement: Scaffold content sourcing

With `--link` flags, the scaffold SHALL be sourced from the multi-source journal for the selected links over the specified window. With `--in-window`, only paragraphs whose lines overlap added-lines in the window SHALL be included. With `--all` (the default) or no window flag, all-time matching paragraphs SHALL be included. With `--from <path>:<line>` (repeatable), the scaffold SHALL additionally include the specified source paragraphs (identified by the line in which they start). Sections in the resulting scaffold SHALL be ordered by journal date descending (newest first), preserving the journal's tiebreak (source title ascending) for equal dates.

The scaffold's per-section body text SHALL be taken verbatim from `JournalEntry.section_text`, which derives from `ParagraphData.text`. Because the heading line remains part of the paragraph that begins at that line (Fork A2), `section_text` is unchanged in shape: a paragraph that begins at a heading line still includes the heading line verbatim.

#### Scenario: --link sources from journal
- **WHEN** `ft notes synth out.md --link "[[Foo]]" --link "[[Bar]]"` is run
- **THEN** the scaffold includes a section for every paragraph that the multi-source journal returns for `Foo` or `Bar`

#### Scenario: --in-window filter applied
- **WHEN** `ft notes synth out.md --link "[[Foo]]" --since 7d --in-window` is run
- **THEN** only paragraphs whose lines overlap added-lines in the last 7 days are included

#### Scenario: --from picks specific paragraphs
- **WHEN** `ft notes synth out.md --link "[[Foo]]" --from notes/bar.md:42 --no-edit` is run
- **THEN** the scaffold includes the journal results for `[[Foo]]` PLUS the paragraph starting at line 42 of `notes/bar.md`

#### Scenario: Scaffold ordered newest first
- **WHEN** the scaffold contains paragraphs dated 2026-03-01 and 2025-11-14
- **THEN** the 2026-03-01 section appears before the 2025-11-14 section in the file

#### Scenario: Paragraph beginning at a heading line includes the heading
- **WHEN** a sourced paragraph begins at a `## Section` heading line
- **THEN** the scaffolded callout body begins with `## Section` (the heading line is part of `ParagraphData.text`, per Fork A2)

### Requirement: Plan/apply split for synth mutations
A pure planner `plan_synth_scaffold(graph, vault, repo, target, entries) -> SynthScaffoldPlan` SHALL compute the file changes without performing any I/O writes. A separate `apply_synth_scaffold(vault, plan)` SHALL perform writes exclusively via `ft_core::fs::write_atomic`. The plan SHALL distinguish create-vs-append cases and SHALL include the frontmatter content (if creating).

#### Scenario: Planner does no I/O
- **WHEN** `plan_synth_scaffold` is invoked
- **THEN** no files on disk are modified

#### Scenario: Applier uses write_atomic
- **WHEN** `apply_synth_scaffold` writes the scaffold
- **THEN** the write goes through `ft_core::fs::write_atomic` (same-dir tempfile + rename)

### Requirement: Synth configuration
The `Config` struct SHALL gain a `synth: Synth` sub-struct with two fields: `folder: String` (default `"Synthesis/"`) and `exclude_prefixes: Vec<String>` (default contains the configured periodic-notes folder prefix when one is configured, else empty). Unknown keys under `[synth]` SHALL be rejected per the existing `deny_unknown_fields` convention.

#### Scenario: Default values applied
- **WHEN** a user has no `[synth]` section in their config
- **THEN** `synth.folder` is `"Synthesis/"` and `synth.exclude_prefixes` defaults to the periodic-notes folder prefix if periodic notes are configured

#### Scenario: User overrides
- **WHEN** a user sets `[synth] folder = "Notes/Synth/"` and `exclude_prefixes = ["Daily/", "Inbox/"]`
- **THEN** the configured values take effect

#### Scenario: Unknown key rejected
- **WHEN** a config has `[synth] unknown_key = "x"`
- **THEN** config load fails with a clear error naming the unknown key

### Requirement: Self-describing synth note targets

A synth note MAY declare its journal target(s) in YAML frontmatter via the key `ft-synth-targets`, whose value SHALL be a YAML sequence of `[[wikilink]]` strings (e.g. `ft-synth-targets: ["[[Foo]]", "[[Bar]]"]`). The key SHALL be optional; notes without it SHALL behave exactly as today (scaffold append, verify, repair, reslice all unchanged). `ft notes synth scaffold` and `ft notes synth grow` SHALL write the key when `--link` is supplied and the note is being created, or when appending and the key is absent. The key SHALL NOT affect verify, repair, or reslice. Parsing SHALL be lenient (accept quoted or bare values, `"[[Foo]]"` or `"Foo"`) and SHALL store values as raw wikilink text. A helper `ft_core::synth::callout::parse_synth_targets(content) -> Option<Vec<String>>` SHALL extract the list; a helper `upsert_synth_frontmatter(content, targets: Option<&[String]>)` SHALL idempotently set both `ft-synth: true` and `ft-synth-targets` without clobbering unrelated frontmatter keys.

#### Scenario: Scaffold writes targets on create

- **WHEN** `ft notes synth scaffold Synthesis/topic.md --link "[[Foo]]" --link "[[Bar]]"` creates a new note
- **THEN** the frontmatter contains `ft-synth: true` and `ft-synth-targets: ["[[Foo]]", "[[Bar]]"]`

#### Scenario: Grow appends targets when key absent

- **WHEN** `ft notes synth grow Synthesis/topic.md --link "[[Baz]]"` is run on an existing note that lacks `ft-synth-targets`
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

