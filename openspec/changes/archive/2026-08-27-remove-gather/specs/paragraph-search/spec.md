# paragraph-search

## MODIFIED Requirements

### Requirement: Scaffold search sourcing

`ft notes synth scaffold <target.md> --search "<query>" [--any] [--sort relevance|date] [--from <path>:<line> ...] [--no-edit]` SHALL source scaffold sections from the search index for the parsed query. The scaffold SHALL require at least one of `--search`, `--link`, or `--from`. Each result SHALL become a `SynthSource` (path, line range, verbatim body); results SHALL be deduplicated by `(source_path, line_start)` before planning. The existing plan/apply path (`plan_synth_scaffold` / `apply_synth_scaffold`, append-dedup, dirty-source guard, `$EDITOR` handoff) SHALL be used unchanged. Sections SHALL emit in result order — relevance descending by default, newest-first with `--sort date`. The transitional `--link "[[X]]"` form SHALL lower to an any-mode search over the given links (Related-alias resolution is not performed on this path). `--from <path>:<line>` SHALL continue to add the specified paragraphs to the section set, unchanged.

#### Scenario: Search sources a new synth note

- **WHEN** `ft notes synth scaffold Synthesis/topic.md --search "eigen ~memoizaton" --no-edit` is run
- **THEN** the note is created with `ft.synth.enabled: true` frontmatter and one protected section per matching paragraph, in relevance order

#### Scenario: Re-running the same search is idempotent

- **WHEN** the same `--search` scaffold is run twice with no source changes
- **THEN** the second run appends zero sections (append-dedup)

#### Scenario: Search plus from picks

- **WHEN** `ft notes synth scaffold Synthesis/topic.md --search "eigen" --from notes/bar.md:42 --no-edit` is run
- **THEN** the scaffold contains the search results plus the paragraph starting at line 42 of `notes/bar.md`

#### Scenario: Link lowering preserves any-of semantics

- **WHEN** `ft notes synth scaffold Synthesis/topic.md --link "[[Foo]]" --link "[[Bar]]" --no-edit` is run
- **THEN** sections cover paragraphs mentioning either link (any-mode, OR over the given links)

#### Scenario: No source flag is an error

- **WHEN** `ft notes synth scaffold Synthesis/topic.md --no-edit` is run with no `--search`, `--link`, or `--from`
- **THEN** the command exits non-zero with a clear "one of --search, --link, or --from is required" error

### Requirement: Search TUI tab

The TUI SHALL register a `Search` tab in the default tab lineup (in the slot the Gather tab previously occupied) with a `<TAB>_COMMANDS`/`<TAB>_KEYMAP` pair, a keymap overlay, a `help_sections()` entry, and a `TestBackend` snapshot. The tab SHALL show an inline input line and a live results list. The results SHALL re-query synchronously on every input change against the snapshot's search index (an `Arc<SearchIndex>` rebuilt on graph generation change; the tab SHALL re-derive on `on_graph_ready` / `on_focus`, never by scanning the vault itself). A status bar SHALL render the live parse: term count, AND/ANY, and sort mode. Keys: `a` SHALL toggle all/any; `o` SHALL cycle sort relevance ↔ date; `Space` SHALL toggle multi-select on the focused row; `Enter` SHALL open the source note in `$EDITOR` at the paragraph's line; `s` SHALL append selected (or all) results to an existing synth note; `S` SHALL create a new synth note from them; `R` SHALL re-run the query. Send-to-synth SHALL reuse the shared synth-send machinery (shared with the Recent tab).

#### Scenario: Live results update per keystroke

- **WHEN** the user types `eig` and then `eigen` in the input line
- **THEN** the results list updates after each keystroke without any explicit reload

#### Scenario: All/any toggle

- **WHEN** the user presses `a` on a two-term query
- **THEN** the status bar flips AND ↔ ANY and the result set updates accordingly

#### Scenario: Sort toggle

- **WHEN** the user presses `o`
- **THEN** the status bar shows the new sort mode and the result order updates
