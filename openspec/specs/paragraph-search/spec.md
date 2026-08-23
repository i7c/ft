# paragraph-search Specification

## Purpose
A scan-derived, paragraph-grain search engine with a small query DSL (substring
default, word, fuzzy, phrase, wikilink, exclude; AND by default, any-mode
optional), relevance and date sorts, a `ft notes search` CLI, a live Search TUI
tab, and scaffold `--search` sourcing. Replaces the graph-based gather as the
sourcing front-end for the synthesis flow; gather is deprecated separately.

## Requirements
### Requirement: Paragraph index built from the scan

A search index SHALL be built from a vault `Scan` — not from the graph and not
from git — over every markdown paragraph captured by `ParsedFile.paragraphs`.
Each indexed document SHALL carry the vault-relative source path, the 1-indexed
inclusive line range, and the verbatim paragraph text. The index SHALL also
store a case-folded copy of the text, a token dictionary (case-folded alphanumeric
runs plus `[[…]]` link tokens) with postings to paragraph ids, and a trigram map
for fuzzy candidate generation. The index SHALL be immutable after build and
SHALL be rebuilt whenever the scan changes (a new scan generation).

#### Scenario: Index covers all paragraphs

- **WHEN** a scan captures two notes with three paragraphs total
- **THEN** the index has three documents, each with its vault-relative path, line range, and text

#### Scenario: Index needs no graph or git

- **WHEN** the index is built in a vault with no git repository
- **THEN** the build succeeds and search results are identical to a git-backed vault (dates excepted)

#### Scenario: Link tokens are extracted from paragraph text

- **WHEN** a paragraph contains `[[Foo Bar]]`, `[[Baz#Section]]`, and `[[Qux|Alias]]`
- **THEN** the index stores link tokens `Foo Bar`, `Baz`, and `Alias` (anchor stripped, alias used)
### Requirement: Query grammar

A single parser SHALL accept the query string used by the CLI argument, the TUI
input line, and scaffold `--search`. The query SHALL parse into an `any` flag and
an ordered list of clauses. A clause SHALL be an optional `-` exclude prefix, an
optional mode prefix, and a term. Modes: no prefix = substring (default),
`=` word, `~` fuzzy, `"…"` phrase, `[[…]]` link. A `[[…]]` span SHALL be atomic:
it may contain spaces and SHALL be scanned before whitespace splitting. An
unterminated `[[` or `"` SHALL degrade to a literal substring term. Space-separated
clauses SHALL AND by default; an explicit any-mode (the `--any` flag on the CLI,
the all/any toggle in the TUI) SHALL make the query OR over its clauses. No `OR`
keyword SHALL be recognized.

#### Scenario: Default substring and AND

- **WHEN** the query `eigen memoization` is parsed
- **THEN** it is two substring clauses ANDed: a paragraph must contain both substrings

#### Scenario: Mode prefixes

- **WHEN** the query `=word ~fuzzy "exact phrase"` is parsed
- **THEN** it is three clauses: a word clause, a fuzzy clause, and a phrase clause, ANDed

#### Scenario: Wikilink term with spaces is atomic

- **WHEN** the query `[[Bar Foo]]` is parsed
- **THEN** it is a single link clause whose term is `Bar Foo` (not two terms)

#### Scenario: Exclude prefix

- **WHEN** the query `eigen -task` is parsed
- **THEN** it is a positive substring clause `eigen` and an excluded substring clause `task`

#### Scenario: Any-mode

- **WHEN** the query `[[foo]] [[bar]]` is parsed with `--any` (or the TUI any toggle)
- **THEN** it is two link clauses ORed: a paragraph mentioning either link qualifies
### Requirement: Matching semantics per mode

Matching SHALL be case-insensitive throughout. A substring clause SHALL match a
paragraph whose case-folded text contains the term as a contiguous sequence. A
phrase clause SHALL match the quoted string as a contiguous sequence of the
case-folded text. A word clause SHALL match a paragraph whose token dictionary
contains the term as a whole token, or whose link tokens contain it. A link
clause SHALL match a paragraph whose link tokens contain the term (the
anchor-stripped or alias-resolved target; case-insensitive). A fuzzy clause
SHALL match paragraphs whose token dictionary contains a token within
levenshtein distance threshold of the term — distance 1 for terms of length 4
or less, else `len/4` — where candidates come from the trigram map and, for
terms of length 3 or less, from a prefix scan of the dictionary. An excluded
clause SHALL be matched with the same mode rules and SHALL remove matching
paragraphs from the result set after positive clauses have been applied.

#### Scenario: Substring matches fragments

- **WHEN** the term `eigen` is searched in substring mode
- **THEN** paragraphs containing `eigen`, `eigenvalue`, or `eigen-decomposition` all match

#### Scenario: Word does not match fragments

- **WHEN** the term `=eigen` is searched
- **THEN** a paragraph containing only `eigenvalue` does NOT match; a paragraph containing the word `eigen` or the link `[[eigen]]` matches

#### Scenario: Fuzzy tolerates typos

- **WHEN** the term `~memoizaton` is searched
- **THEN** paragraphs containing the token `memoization` match

#### Scenario: Link clause restricts to link targets

- **WHEN** the term `[[memoization]]` is searched
- **THEN** a paragraph containing `[[Memoization]]` matches even if the word `memoization` never appears in its prose

#### Scenario: Exclude filters after matching

- **WHEN** the query `eigen -task` is run
- **THEN** a paragraph containing `eigen` and `task` is excluded; a paragraph containing only `eigen` is included
### Requirement: Relevance ranking and deterministic order

The default sort SHALL order results by relevance score descending. The score
SHALL be the sum over matched positive clauses of: a mode weight (phrase 3,
word 2, link 2, substring 1.5, fuzzy 1) multiplied by an occurrence boost
(1 + 0.5 × (occurrences − 1), capped at a factor of 3) and a position bonus
favoring the clause's first hit earlier in the paragraph. Ties SHALL break by
vault-relative path ascending, then line start ascending. The same query over
the same index SHALL produce identical ordering across runs.

#### Scenario: More clauses rank higher

- **WHEN** one paragraph matches two clauses and another matches one
- **THEN** the two-clause paragraph ranks first

#### Scenario: Deterministic tiebreak

- **WHEN** two paragraphs have equal scores
- **THEN** the one with the lexicographically smaller vault-relative path ranks first, then the smaller line start
### Requirement: Date sort via blame

`--sort date` SHALL order results by the paragraph's most recent `git blame`
date, descending (newest first), breaking ties by relevance score then path then
line. Date lookup SHALL use the existing `BlameCache` and SHALL blame only the
result-set files, lazily; paragraphs whose blame fails SHALL sort as oldest
(matching the gather feed's degradation for untracked files).

#### Scenario: Newest edit first

- **WHEN** two paragraphs match and one was last edited more recently
- **THEN** the more recently edited paragraph ranks first under `--sort date`
### Requirement: ft notes search command

`ft notes search <query> [--any] [--sort relevance|date] [--limit N] [--json]` SHALL be a subcommand of `ft notes`. The default sort SHALL be `relevance`.
The command SHALL respect `synth.exclude_prefixes` (files whose vault-relative
path starts with a configured prefix SHALL NOT be searched). Text output SHALL
print one result per line: vault-relative path, `L<start>-<end>`, the matched
clause labels, and the paragraph text; colors SHALL auto-disable off a TTY.
`--json` SHALL print an array of objects with `path`, `line_start`, `line_end`,
`body`, `matched` (array of clause labels), `score`, and `date` (present only
with `--sort date`). An empty query or an empty result set SHALL print nothing
and exit 0.

#### Scenario: Search returns matched paragraphs

- **WHEN** `ft notes search eigen` is run in a vault whose paragraph mentions `eigenvalue`
- **THEN** the paragraph is printed with its vault-relative path, line range, and matched clause label

#### Scenario: JSON shape

- **WHEN** `ft notes search eigen --json` is run
- **THEN** stdout is a JSON array where each element has `path`, `line_start`, `line_end`, `body`, `matched`, and `score`

#### Scenario: Exclude prefixes respected

- **WHEN** the config sets `[synth] exclude_prefixes = ["journal/"]` and the only match is under `journal/`
- **THEN** the command prints nothing and exits 0
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
- **THEN** sections cover paragraphs mentioning either link (any-mode), matching the deprecated gather's multi-target behavior

#### Scenario: No source flag is an error

- **WHEN** `ft notes synth scaffold Synthesis/topic.md --no-edit` is run with no `--search`, `--link`, or `--from`
- **THEN** the command exits non-zero with a clear "one of --search, --link, or --from is required" error
### Requirement: Search TUI tab

The TUI SHALL register a `Search` tab in the default tab lineup (in the slot the Gather tab previously occupied) with a `<TAB>_COMMANDS`/`<TAB>_KEYMAP` pair, a keymap overlay, a `help_sections()` entry, and a `TestBackend` snapshot. The tab SHALL show an inline input line and a live results list. The results SHALL re-query synchronously on every input change against the snapshot's search index (an `Arc<SearchIndex>` rebuilt on graph generation change; the tab SHALL re-derive on `on_graph_ready` / `on_focus`, never by scanning the vault itself). A status bar SHALL render the live parse: term count, AND/ANY, and sort mode. Keys: `a` SHALL toggle all/any; `o` SHALL cycle sort relevance ↔ date; `Space` SHALL toggle multi-select on the focused row; `Enter` SHALL open the source note in `$EDITOR` at the paragraph's line; `s` SHALL append selected (or all) results to an existing synth note; `S` SHALL create a new synth note from them; `R` SHALL re-run the query. Send-to-synth SHALL reuse the shared synth-send machinery (extracted from the Gather tab).

#### Scenario: Live results update per keystroke

- **WHEN** the user types `eig` and then `eigen` in the input line
- **THEN** the results list updates after each keystroke without any explicit reload

#### Scenario: All/any toggle

- **WHEN** the user presses `a` on a two-term query
- **THEN** the status bar flips AND ↔ ANY and the result set updates accordingly

#### Scenario: Sort toggle

- **WHEN** the user presses `o`
- **THEN** the status bar shows the new sort mode and the result order updates

#### Scenario: Send selected to synth

- **WHEN** the user selects rows with `Space` and presses `s`
- **THEN** the synth-note picker opens with only the selected paragraphs as sources

#### Scenario: Enter opens the source

- **WHEN** the user presses `Enter` on a result
- **THEN** `$EDITOR` opens the source note at the paragraph's line start
### Requirement: Pulse handoff to Search

Pressing `Enter` on the Pulse tab with selected (or cursor) rows SHALL switch to the Search tab with an input prefilled with one `[[<target>]]` clause per selected row and any-mode enabled (gather-parity: any of the links qualifies). The handoff SHALL use an app-level request (`AppRequest::SearchWithQuery { query, any: true }`), not a gather handoff.

#### Scenario: Handoff prefills links in any-mode

- **WHEN** the Pulse tab shows rows for `[[Foo]]` and `[[Bar]]`, the user selects both, and presses `Enter`
- **THEN** the Search tab opens with the query `[[Foo]] [[Bar]]` and any-mode active
### Requirement: Search performance budget

Index build from a scan SHALL be a single pass over the captured paragraphs (no re-reads, no git). Under the `FT_PERF_TESTS=1` gate, a query over a 5,000-paragraph vault SHALL complete in under 10 ms end-to-end (parse + match + rank + limit), and fuzzy queries SHALL examine only trigram-filtered candidates rather than the full dictionary.

#### Scenario: Perf gate holds

- **WHEN** `FT_PERF_TESTS=1` is set and a 5,000-paragraph fixture vault is searched with a substring query and a fuzzy query
- **THEN** each query completes within the 10 ms budget
