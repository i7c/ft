# paragraph-search Specification

## Purpose
A scan-derived, paragraph-grain search engine with a small query DSL (substring
default, word, fuzzy, phrase, wikilink, exclude; AND by default, any-mode
optional), relevance and date sorts, a `ft notes search` CLI, a live Search TUI
tab, and scaffold `--search` sourcing. Replaces the graph-based gather as the
sourcing front-end for the synthesis flow; gather is deprecated separately.
## Requirements
### Requirement: Paragraph index built from the scan

A search index SHALL be built from a vault `Scan` — not from git — over every
markdown paragraph captured by `ParsedFile.paragraphs`. Each indexed document
SHALL carry the vault-relative source path, the 1-indexed inclusive line range,
the verbatim paragraph text, and its **context fields**: the note title (the
file stem), the note's frontmatter aliases (`aliases` / `alias`), the heading
path (the text of the ATX headings enclosing the paragraph, outermost first),
and the paragraph's tag set (the note's frontmatter `tags` / `tag` plus inline
`#tags` written in the paragraph itself). Matching surfaces SHALL be folded:
lowercased and diacritic-stripped (Unicode NFD with combining marks removed),
applied identically to index and query text. Word tokens SHALL be maximal runs
of Unicode alphanumeric characters of the folded text; `[[…]]` spans SHALL
contribute their anchor-stripped, alias-resolved target as a link token. The
index SHALL store, per field, a token dictionary with postings carrying term
frequency, per-document field lengths, a trigram map over the dictionary for
fuzzy candidates, and a stem map per configured stemmer. The index SHALL be
immutable after build and SHALL be rebuilt whenever the scan changes (a new scan
generation). The build SHALL derive per-note ranking priors and the undirected
note-link adjacency from the scan's own links, resolved with the link graph's
resolution rules (`graph::resolve`), without building the graph.

#### Scenario: Index covers all paragraphs

- **WHEN** a scan captures two notes with three paragraphs total
- **THEN** the index has three documents, each with its vault-relative path, line range, text, and context fields

#### Scenario: Index needs no graph or git

- **WHEN** the index is built in a vault with no git repository
- **THEN** the build succeeds without building the link graph, and text-clause results are identical to a git-backed vault (dates excepted)

#### Scenario: Link tokens are extracted from paragraph text

- **WHEN** a paragraph contains `[[Foo Bar]]`, `[[Baz#Section]]`, and `[[Qux|Alias]]`
- **THEN** the index stores link tokens `foo bar`, `baz`, and `alias` (folded, anchor stripped, alias used)

#### Scenario: Accented words are whole tokens

- **WHEN** a paragraph contains `o ônibus da manhã`
- **THEN** the dictionary contains the tokens `onibus` and `manha`, not the fragments `nibus` or `manh`

#### Scenario: Heading path is captured

- **WHEN** a paragraph sits under `## Design` which sits under `# Busão`
- **THEN** the paragraph's heading path is `Busão › Design` and its folded heading tokens include `busao` and `design`

#### Scenario: Tag set combines frontmatter and inline tags

- **WHEN** a note has frontmatter `tags: [proj/busao]` and one of its paragraphs contains `#idea`
- **THEN** that paragraph's tag set is `proj/busao` and `idea`, and the note's other paragraphs carry only `proj/busao`

### Requirement: Query grammar

A single parser SHALL accept the query string used by the CLI argument, the TUI
input line, and scaffold `--search`. The query SHALL parse into an `any` flag and
an ordered list of clauses. A **text clause** SHALL be an optional `-` exclude
prefix, an optional mode prefix, and a term. Modes: no prefix = substring
(default), `=` word, `~` fuzzy, `"…"` phrase, `[[…]]` link. A `[[…]]` span SHALL
be atomic: it may contain spaces and SHALL be scanned before whitespace
splitting. An unterminated `[[` or `"` SHALL degrade to a literal substring
term. A **filter clause** SHALL be an optional `-` prefix and one of the keys
`path:`, `tag:`, `after:`, `before:`, `scope:` immediately followed by a
non-empty value (a bare token or a `"…"`-quoted string); a **boost clause** SHALL
be `near:` followed by a `[[…]]` span. A `key:` prefix whose key is not one of
these, or whose value is empty, SHALL parse as a literal substring term. Text
clauses SHALL AND by default; an explicit any-mode (the `--any` flag on the CLI,
the all/any toggle in the TUI) SHALL make the query OR over its positive text
clauses only — filter clauses always AND. No `OR` keyword SHALL be recognized.
Parsing SHALL be total and SHALL never fail; the canonical render of a parsed
query SHALL re-parse to the same query.

#### Scenario: Default substring and AND

- **WHEN** the query `eigen memoization` is parsed
- **THEN** it is two substring clauses ANDed: a paragraph must match both

#### Scenario: Mode prefixes

- **WHEN** the query `=word ~fuzzy "exact phrase"` is parsed
- **THEN** it is three clauses: a word clause, a fuzzy clause, and a phrase clause, ANDed

#### Scenario: Wikilink term with spaces is atomic

- **WHEN** the query `[[Bar Foo]]` is parsed
- **THEN** it is a single link clause whose term is `bar foo` (not two terms)

#### Scenario: Exclude prefix

- **WHEN** the query `eigen -task` is parsed
- **THEN** it is a positive substring clause `eigen` and an excluded substring clause `task`

#### Scenario: Any-mode

- **WHEN** the query `[[foo]] [[bar]]` is parsed with `--any` (or the TUI any toggle)
- **THEN** it is two link clauses ORed: a paragraph mentioning either link qualifies

#### Scenario: Filter and boost clauses

- **WHEN** the query `cache path:"Projects/Big Idea/" -tag:archive after:-30d near:[[Caching]]` is parsed
- **THEN** it is a substring clause `cache`, a path filter, a negated tag filter, a date filter, and a near boost

#### Scenario: Unknown key stays literal

- **WHEN** the query `http://example.com foo:bar path:` is parsed
- **THEN** all three are substring clauses (`http://example.com`, `foo:bar`, `path:`)

#### Scenario: Filters stay conjunctive in any-mode

- **WHEN** the query `alpha beta path:notes/` is run with `--any`
- **THEN** results contain paragraphs matching `alpha` or `beta`, all of them under `notes/`

### Requirement: Matching semantics per mode

Matching SHALL be case- and diacritic-insensitive throughout. A positive text
clause SHALL match a paragraph when it matches the paragraph's body or any of
its context fields (title, aliases, heading path, tags), except link clauses,
which SHALL match the body's link tokens only. An excluded text clause SHALL
match the body only. A substring clause SHALL match a field whose folded text
contains the term as a contiguous sequence. A phrase clause SHALL match the
quoted string as a contiguous sequence of a field's folded text. A word clause
SHALL match a field containing the term as a whole token, or containing a token
with the same stem under any configured stemmer (see "Search configuration"),
or containing it as a link token. A link clause SHALL match a paragraph whose
body link tokens contain the term (the anchor-stripped or alias-resolved
target). A fuzzy clause SHALL match fields containing a token within levenshtein
distance threshold of the term — distance 1 for terms of 4 characters or fewer,
else `chars/4`, counted in characters — where candidates come from the trigram
map and, for terms of 3 characters or fewer, from a prefix scan of the
dictionary. Excluded clauses SHALL remove matching paragraphs after positive
clauses have been applied.

#### Scenario: Substring matches fragments

- **WHEN** the term `eigen` is searched in substring mode
- **THEN** paragraphs containing `eigen`, `eigenvalue`, or `eigen-decomposition` all match

#### Scenario: Word does not match fragments

- **WHEN** the term `=eigen` is searched
- **THEN** a paragraph containing only `eigenvalue` does NOT match; a paragraph containing the word `eigen` or the link `[[eigen]]` matches

#### Scenario: Word matches accented text both ways

- **WHEN** a paragraph contains `ônibus` and the queries `=ônibus` and `=onibus` are run
- **THEN** both queries return the paragraph

#### Scenario: Word matches stem variants

- **WHEN** stemmers are `["english"]` and a paragraph contains only `memories`
- **THEN** the query `=memory` returns the paragraph

#### Scenario: Fuzzy tolerates typos

- **WHEN** the term `~memoizaton` is searched
- **THEN** paragraphs containing the token `memoization` match

#### Scenario: Link clause restricts to link targets

- **WHEN** the term `[[memoization]]` is searched
- **THEN** a paragraph containing `[[Memoization]]` matches even if the word `memoization` never appears in its prose, and a paragraph in the note `Memoization.md` that does not link it does not match

#### Scenario: Context fields qualify a paragraph

- **WHEN** the note `Memoization.md` has a paragraph mentioning `eigen` but not `memoization`, and the query `eigen memoization` is run
- **THEN** that paragraph matches (body satisfies `eigen`, title satisfies `memoization`)

#### Scenario: Exclude filters after matching on the body only

- **WHEN** the query `eigen -task` is run against the note `Task Notes.md` whose paragraph contains `eigen` but not `task`
- **THEN** the paragraph is included; a paragraph whose body contains both `eigen` and `task` is excluded

### Requirement: Relevance ranking and deterministic order

The default sort SHALL order results by relevance score descending. The score
SHALL be the BM25F text score multiplied by the note prior and the near boost
(see "Graph and recency priors"). The BM25F text score SHALL be the sum over
matched positive text clauses of the clause's mode weight times the clause's
best BM25F contribution among the dictionary tokens it resolved to in that
paragraph, where BM25F combines per-field term frequencies with per-field
weights and per-field length normalization, and uses an inverse document
frequency over paragraphs. Phrase and multi-token substring clauses SHALL use
their occurrence counts as term frequency. A clause that matches a paragraph only
through context fields SHALL contribute a damped score, so a paragraph's own
words outweigh text it merely inherits from its note. Mode weights SHALL rank
exact intent above approximate (phrase ≥ word = link ≥ substring ≥ fuzzy). Field
weights, `k1`, `b`, the context-only damping, and mode weights SHALL be named
constants tuned against the relevance evaluation harness. Ties SHALL break by vault-relative path ascending,
then line start ascending. The same query over the same index SHALL produce
identical ordering across runs.

#### Scenario: More clauses rank higher

- **WHEN** one paragraph matches two clauses and another matches one, with comparable term statistics
- **THEN** the two-clause paragraph ranks first

#### Scenario: Focused paragraph beats a long list

- **WHEN** a three-line paragraph mentions `memory` once and a thirty-line bullet list mentions it three times
- **THEN** the three-line paragraph ranks first

#### Scenario: Rare terms outweigh common terms

- **WHEN** the query `the eigendecomposition` is run with `--any` and one paragraph contains only `the` while another contains only `eigendecomposition`
- **THEN** the `eigendecomposition` paragraph ranks first

#### Scenario: Title match outranks body-only match

- **WHEN** two paragraphs mention `eigen` equally in their bodies and only one sits in the note `Eigen.md`
- **THEN** the paragraph in `Eigen.md` ranks first

#### Scenario: Title-only matches do not flood the results

- **WHEN** the note `Short-term-memory.md` has ten paragraphs none of which mention `memory`, and a paragraph in another note mentions `memory` once
- **THEN** the paragraph that mentions `memory` ranks above every paragraph matched only through the title

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

`ft notes search <query> [--any] [--sort relevance|date] [--limit N] [--flat] [--json]` SHALL be a subcommand of `ft notes`. The default sort SHALL be `relevance`.
The command SHALL build the index exactly as the TUI snapshot does, so CLI and
TUI rank identically, and SHALL build the link graph only when the query has a
`scope:` filter. The command SHALL respect
`synth.exclude_prefixes` (files whose vault-relative path starts with a
configured prefix SHALL NOT be searched). `--limit N` SHALL cap the number of
paragraphs before grouping. Default text output SHALL group results by note:
groups ordered by their best member (score under relevance, date under date
sort), members in result order; each group SHALL print a header with the
vault-relative path and the note title (omitted when equal to the file stem),
followed by one indented row per paragraph with `L<start>-<end>`, the heading
path, the matched clause labels, and a snippet — a window of at most 160
characters around the first highlight, trimmed to word boundaries, with
ellipses marking cuts, and highlights emphasized when color is on. `--flat`
SHALL print one result per line: vault-relative path, `L<start>-<end>`, the
matched clause labels, and the paragraph text. Colors SHALL auto-disable off a
TTY. `--json` SHALL print a flat array of objects with `path`, `line_start`,
`line_end`, `body`, `matched` (array of clause labels), `score`, `title`,
`heading_path` (array of strings), `highlights` (array of `[start, end)` byte
ranges into `body`), `reasons` (array of `{clause, field}` objects, `field` one
of `body`, `title`, `alias`, `heading`, `tag`), and `date` (present only with
`--sort date` or a date filter). An empty query or an empty result set SHALL
print nothing and exit 0.

#### Scenario: Search returns matched paragraphs grouped by note

- **WHEN** `ft notes search eigen` is run in a vault where two paragraphs of `notes/linear.md` mention `eigenvalue`
- **THEN** stdout shows one `notes/linear.md` group header followed by two indented rows, each with its line range, matched clause label, and a snippet

#### Scenario: Flat output

- **WHEN** `ft notes search eigen --flat` is run
- **THEN** each matching paragraph prints on its own line with path, line range, matched clause labels, and paragraph text

#### Scenario: JSON shape

- **WHEN** `ft notes search eigen --json` is run
- **THEN** stdout is a JSON array where each element has `path`, `line_start`, `line_end`, `body`, `matched`, `score`, `title`, `heading_path`, `highlights`, and `reasons`

#### Scenario: Highlights point into the body

- **WHEN** `ft notes search ônibus --json` matches a body containing `Ônibus`
- **THEN** a `highlights` range slices exactly `Ônibus` out of `body`

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
- **THEN** sections cover paragraphs mentioning either link (any-mode, OR over the given links)

#### Scenario: No source flag is an error

- **WHEN** `ft notes synth scaffold Synthesis/topic.md --no-edit` is run with no `--search`, `--link`, or `--from`
- **THEN** the command exits non-zero with a clear "one of --search, --link, or --from is required" error

### Requirement: Search TUI tab

The TUI SHALL register a `Search` tab in the default tab lineup with a `<TAB>_COMMANDS`/`<TAB>_KEYMAP` pair, a keymap overlay, a `help_sections()` entry, and a `TestBackend` snapshot. The tab SHALL show an inline input line and a live results list. The results SHALL re-query synchronously on every input change against the snapshot's search index (an `Arc<SearchIndex>` built from the snapshot's scan and rebuilt on graph generation change, with the snapshot's graph serving `scope:`; the tab SHALL re-derive on `on_graph_ready` / `on_focus`, never by scanning the vault itself). A status bar SHALL render the live parse: term count, filter count, AND/ANY, sort mode, and grouping mode. Results SHALL be grouped by note by default, with one non-focusable-for-open header row per note; each paragraph row SHALL show its line range, heading path, and matched clause labels; the preview pane SHALL render the focused paragraph with every highlight range emphasized. Keys: `a` SHALL toggle all/any; `o` SHALL cycle sort relevance ↔ date; `g` SHALL toggle grouped ↔ flat; `Space` SHALL toggle multi-select on the focused row (on a group header: all of that note's rows); `Enter` SHALL open the source note in `$EDITOR` at the paragraph's line (on a group header: at the first member's line); `s` SHALL append selected (or all) results to an existing synth note; `S` SHALL create a new synth note from them; `R` SHALL re-run the query. Send-to-synth SHALL reuse the shared synth-send machinery and SHALL ship paragraphs in result order regardless of grouping. A query whose `scope:` names an unknown preset SHALL show the error in the status bar and an empty result list.

#### Scenario: Live results update per keystroke

- **WHEN** the user types `eig` and then `eigen` in the input line
- **THEN** the results list updates after each keystroke without any explicit reload

#### Scenario: All/any toggle

- **WHEN** the user presses `a` on a two-term query
- **THEN** the status bar flips AND ↔ ANY and the result set updates accordingly

#### Scenario: Sort toggle

- **WHEN** the user presses `o`
- **THEN** the status bar shows the new sort mode and the result order updates

#### Scenario: Grouping toggle

- **WHEN** results span two notes and the user presses `g`
- **THEN** the note header rows disappear and the list shows one row per paragraph; pressing `g` again restores the headers

#### Scenario: Select a whole note

- **WHEN** the cursor is on a group header with three member rows and the user presses `Space`
- **THEN** all three paragraphs are selected

#### Scenario: Preview highlights matches

- **WHEN** the focused paragraph matches the query `cache` twice
- **THEN** the preview pane renders both occurrences with the highlight style

### Requirement: Search performance budget

Index build from a scan SHALL be a single pass over the captured paragraphs and
frontmatter (no re-reads, no git, no graph build); deriving priors and adjacency
SHALL be linear in the number of links. Under the `FT_PERF_TESTS=1` gate, a query over a
5,000-paragraph vault SHALL complete in under 10 ms end-to-end (parse + match +
rank + highlight + limit), excluding date filters and date sort (which blame
lazily), and fuzzy queries SHALL examine only trigram-filtered candidates rather
than the full dictionary.

#### Scenario: Perf gate holds

- **WHEN** `FT_PERF_TESTS=1` is set and a 5,000-paragraph fixture vault is searched with a substring query, a fuzzy query, and a query with a `path:` filter and a `near:` boost
- **THEN** each query completes within the 10 ms budget

### Requirement: Filter clauses

Filter clauses SHALL restrict the result set and SHALL NOT contribute to the
score. `path:<prefix>` SHALL keep paragraphs whose folded vault-relative path
starts with the folded prefix. `tag:<tag>` SHALL keep paragraphs whose tag set
contains the tag or a descendant of it (`tag:proj` keeps `proj` and
`proj/busao`); a leading `#` on the value SHALL be ignored. `after:<date>` and
`before:<date>` SHALL keep paragraphs whose blame date is on or after / on or
before the date; dates SHALL accept the graph-DSL date values (`YYYY-MM-DD`,
`today`, `yesterday`, `tomorrow`, `±Nd`, `±Nw`, `±Nm`) resolved against
`ft_core::dates::today()` (so `FT_TODAY` applies); an unparseable date value
SHALL make the clause a literal substring term. Date filters SHALL blame only
the files of paragraphs that survive every other clause, through the shared
`BlameCache`; paragraphs without a blame date (untracked, no git) SHALL NOT pass
a date filter, and the CLI SHALL print one stderr warning naming the count of
undated paragraphs dropped. `scope:<preset>` SHALL keep paragraphs whose note is
selected by the named graph preset (user `[graph.presets]` shadowing
built-ins), evaluated against the same graph; an unknown preset SHALL be a
search error — the CLI and `synth scaffold --search` SHALL exit 2 naming the
preset and listing the available names. A `-` prefix SHALL invert any filter. A
query consisting only of filter clauses SHALL return every paragraph passing
them, ranked by prior then path then line.

#### Scenario: Path filter

- **WHEN** the query `cache path:proj/` is run
- **THEN** only paragraphs from files under `proj/` are returned

#### Scenario: Hierarchical tag filter

- **WHEN** the query `tag:proj` is run in a vault with notes tagged `proj/busao` and `proj/fluxo`
- **THEN** paragraphs from both notes are returned

#### Scenario: Negated filter

- **WHEN** the query `cache -path:archive/` is run
- **THEN** no returned paragraph comes from a file under `archive/`

#### Scenario: Date filter with FT_TODAY

- **WHEN** `FT_TODAY=2026-10-23` and the query `cache after:-7d` is run
- **THEN** only paragraphs whose blame date is on or after 2026-10-16 are returned

#### Scenario: Date filter without git

- **WHEN** the query `cache after:2026-01-01` is run in a vault with no git repository
- **THEN** no paragraphs are returned and the CLI prints one warning that the matching paragraphs had no blame date

#### Scenario: Scope via graph preset

- **WHEN** the user config defines graph preset `work` selecting notes under `proj/`, and the query `cache scope:work` is run
- **THEN** only paragraphs from notes the preset selects are returned

#### Scenario: Unknown scope preset

- **WHEN** `ft notes search "cache scope:nope"` is run
- **THEN** the command exits 2 and stderr names `nope` and lists the available preset names

#### Scenario: Filter-only query

- **WHEN** the query `path:proj/ tag:idea` is run
- **THEN** every paragraph under `proj/` whose tag set includes `idea` is returned

### Requirement: Graph and recency priors

Each result's score SHALL be multiplied by a prior in `[1.0, 1.25]` made of two
parts. (a) A **note part** in `[0, 0.15]` from the note's in-degree — the number
of distinct other scanned notes whose links resolve to it (links from files
excluded from the index still count) — log-scaled against the vault's maximum
in-degree. (b) A **paragraph part** in `[0, 0.10]` from the paragraph's recency
date with exponential decay over whole days relative to the newest recency date
among indexed paragraphs (not wall-clock "today", so copies of a vault rank
identically). A paragraph's recency date SHALL be its blame date (the newest
commit touching its line range) when the blame cache holds a valid entry for its
file, and otherwise the date of its file's mtime. Recency dates SHALL be supplied
at query time through a recency table, so new blame data changes ranking without
rebuilding the index; without a table, every paragraph SHALL use its file's
mtime date. A
`near:[[Note]]` clause SHALL multiply the score of paragraphs by a boost that
decreases with undirected link-graph distance from the resolved seed note —
`1.5` for the seed itself, `1.3` at distance 1, `1.15` at distance 2, `1.0`
beyond. An unresolved seed SHALL apply no boost and SHALL NOT error. `near:`
SHALL never filter; a query whose only clauses are `near:` boosts SHALL return
nothing. Priors and boosts SHALL only reorder results that the text and filter
clauses already selected.

#### Scenario: Hub note ranks above an orphan on a tie

- **WHEN** two paragraphs have equal BM25F scores and one note has twenty backlinks while the other has none
- **THEN** the paragraph in the linked note ranks first

#### Scenario: Prior is bounded

- **WHEN** a paragraph with a strong text match sits in an orphan note and a paragraph with a text score less than 0.8 times as high sits in the most-linked note and was edited most recently
- **THEN** the strong text match still ranks first

#### Scenario: Fresh paragraph in an old note

- **WHEN** one file holds two equally matching paragraphs, its blame shows one edited today and the other three years ago, and the blame cache holds the file
- **THEN** the paragraph edited today ranks first

#### Scenario: Uncached file falls back to mtime

- **WHEN** the blame cache has no valid entry for a file
- **THEN** every paragraph of that file uses the file's mtime date as its recency date, and the search succeeds without invoking `git blame`

#### Scenario: Near boost reorders without filtering

- **WHEN** the query `cache near:[[Caching]]` is run and paragraphs mention `cache` in `Caching.md`, in a note linked from it, and in an unrelated note
- **THEN** all three are returned, with the boost applied to the first two

#### Scenario: Unresolved near seed

- **WHEN** the query `cache near:[[Does Not Exist]]` is run
- **THEN** results equal those of `cache` alone, in the same order

### Requirement: Search configuration

A `[search]` config table SHALL accept `stemmers`, a list of Snowball algorithm
names (e.g. `"english"`, `"portuguese"`, `"german"`), defaulting to
`["english"]`; `[]` SHALL disable stemming. An unknown name SHALL be a config
load error naming the bad value and listing the valid names. Stemming SHALL
apply to word clauses only; substring, phrase, fuzzy, and link clauses SHALL be
unaffected.

#### Scenario: Multiple stemmers

- **WHEN** `stemmers = ["english", "portuguese"]` and one paragraph contains only `memories` and another only `casas`
- **THEN** `=memory` returns the first paragraph and `=casa` returns the second

#### Scenario: Stemming disabled

- **WHEN** `stemmers = []` and a paragraph contains only `memories`
- **THEN** `=memory` does not return it

#### Scenario: Unknown stemmer

- **WHEN** `stemmers = ["klingon"]`
- **THEN** loading the config fails with an error naming `klingon` and listing the valid algorithm names

### Requirement: Recency table loading

`ft notes search`, `ft notes synth scaffold --search`, and the TUI SHALL build
the recency table from the on-disk blame cache and one validity snapshot (blob
ids from `git ls-tree -r HEAD` and dirty paths from `git status`), without
invoking `git blame` for a plain relevance search. Outside a git repository the
table SHALL be empty (every paragraph falls back to mtime) and the search SHALL
succeed.

#### Scenario: Plain search does not blame

- **WHEN** `ft notes search cache` runs in a git vault with a cold blame cache
- **THEN** no `git blame` subprocess is spawned and results are ranked with mtime recency

#### Scenario: Warm cache changes ranking without reindexing

- **WHEN** the TUI's blame warm-up finishes after the Search tab already shows results
- **THEN** the tab re-ranks the current query with the new recency table without a vault rescan

### Requirement: Search near the focused result

The Search tab SHALL bind `n` to `search.near-focused`: it SHALL set the query's
`near:` clause to the note of the focused result (`near:[[<title>]]`, replacing
any existing `near:` clause, keeping every other clause and the any-mode flag),
update the input line, and re-run the query. On an empty result list it SHALL do
nothing. The command SHALL appear in `SEARCH_COMMANDS`, `SEARCH_KEYMAP`, the `?`
overlay, and `docs/keybindings.md`.

#### Scenario: Pin the neighbourhood of a result

- **WHEN** the query is `cache` and the focused result is in `Caching.md`, and the user presses `n`
- **THEN** the query becomes `cache near:[[Caching]]` and the results re-rank with the boost

#### Scenario: Replace an existing near clause

- **WHEN** the query is `cache near:[[Other]]` and the focused result is in `Caching.md`, and the user presses `n`
- **THEN** the query becomes `cache near:[[Caching]]`

