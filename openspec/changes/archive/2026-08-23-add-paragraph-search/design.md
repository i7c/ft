# Design: paragraph search replaces gather as the synth sourcing front-end

## Context

Synthesis today sources its raw material through `ft_core::gather` — a graph
walk over `ParagraphLink` edges with blame dates, Related-alias resolution
(single-target), heading-section expansion, and multi-target `matched`
attribution. It exists to answer "which paragraphs mention `[[X]]` (or a set
of links)?". The vault scan already carries everything a simpler answer needs:
`ParsedFile.paragraphs` (text + line range) and the raw wikilinks. A search
index over those paragraphs generalizes the question — any term, not just
links — and adds the cross-reference query (AND terms in one paragraph) the
graph walk cannot express, at live as-you-type latency.

The plan is staged: this change adds the search engine + CLI + TUI tab,
rewires scaffold sourcing and the pulse handoff, removes `grow`, and
deprecates (hides) gather. A follow-up change deletes the gather engine, the
Gather tab, and the citation layer once search has proven out.

Constraints from the workspace: plan/apply split for all mutations; the TUI is
single-threaded except two producer threads (the search index is read-only
after build, so queries are synchronous); the CLI is the contract for
ft.nvim (which consumes only `quote`/`export` — untouched); `recent` and
`reslice` are out of scope and keep blame + the citation machinery alive.

## Goals / Non-Goals

**Goals:**
- A paragraph-grain search engine with the confirmed DSL: substring default,
  `=word`, `~fuzzy`, `"phrase"`, `[[link]]` (spaces allowed, atomic), `-exclude`;
  AND by default, `--any` for OR; `--sort relevance|date`.
- Live as-you-type results in the TUI (synchronous query over an in-memory
  index; no worker round-trip per keystroke).
- `ft notes search` CLI with stable `--json` output for scripts/plugins.
- `ft notes synth scaffold --search "<query>"` with `--any`/`--sort`, reusing
  the existing plan/apply + append-dedup + dirty-source machinery unchanged.
- Pulse handoff → Search tab (prefilled `[[…]]` terms, any-mode).
- Deprecation surface: gather/journal hidden + warning; Gather tab opt-in via
  `[tui] show_gather`; scaffold `--link`/`--from` transitional.
- Remove `grow` (subcommand, `--new-only`, `--limit`, watermark,
  self-describing `ft.synth.targets` frontmatter) and the scaffold window
  flags (`--in-window`, `--since`, `--range`).

**Non-Goals:**
- Mixed AND/OR in one query (one logical operator per query; parenthesized
  groups are a future extension).
- Folder-scoped search, stemming, persisted/on-disk index, result snippets
  with term highlighting (the paragraph is the result unit).
- Deleting the gather engine, Gather tab code, or the citation layer
  (`--uncited`, badges) — deferred to the follow-up removal change.
- Any change to `recent`, `reslice`, `verify`, `repair`, `quote`, `export`,
  pulse's engine, or the callout grammar.

## Decisions

### D1. Index is built from the scan, not the graph
Corpus: `ParsedFile.paragraphs` flattened to `ParagraphDoc { path, line_start,
line_end, text }`, plus a lowercased text cache. Rationale: the scan is the
shared, cheaper artifact (the graph re-derives the same paragraphs); search
needs no edges, no headings, no git. `Graph::build` and `SearchIndex::build`
become siblings over the same `Scan`. The TUI holds the index in the shared
snapshot (`Arc<SearchIndex>` beside `Arc<GraphSnapshot>`), rebuilt on
generation change by the same background worker that rebuilds the graph.

### D2. Link tokens are extracted from paragraph text, not from `RawLink`s
The scan's per-file `RawLink` list is not per-paragraph. Instead the
tokenizer recognizes `[[…]]` runs inside each paragraph's text directly:
the bracketed content is one token, `[[Foo Bar]]` is atomic with its space,
`#anchor` is stripped, `[[X|Alias]]` matches on `Alias`. This keeps link
matching self-contained per paragraph and needs no new scan data.

### D3. Four match units over two structures
- **Substring / phrase**: case-insensitive contains over the lowercased text
  cache (phrase = the quoted string as a contiguous sequence). A raw scan is
  sub-ms at vault scale (5k paragraphs × ~300 chars); no index needed. A
  perf gate (`FT_PERF_TESTS=1`) asserts the budget; a trigram substring index
  is the documented fallback if a vault outgrows it.
- **Word / link**: inverted token index — token → sorted paragraph ids.
  Tokens are alphanumeric runs plus `[[…]]` link tokens. `=foo` matches token
  `foo` or link target `foo`; `[[Foo]]` matches link tokens only.
- **Fuzzy**: trigram dictionary — trigram → dictionary tokens; query-side
  trigrams of the term produce candidate tokens, filtered by levenshtein
  (threshold: 1 for terms ≤ 4 chars, else `len/4`), then postings.
  Terms ≤ 3 chars skip trigrams and scan the dictionary by prefix to avoid
  empty recall. Case-insensitive throughout.
- **Exclude**: `-clause` matches with the same modes, then filters.

### D4. Query DSL is one parser, three surfaces
`search::query::parse(&str) -> Query { any: bool, clauses: Vec<Clause> }`.
Clauses are scanned in one pass: `[[…]]` (atomic, may contain spaces),
`"…"` (phrase), then mode prefixes `~`/`=`/`-` before a whitespace-delimited
term. Unmatched `[[` or `"` degrades to a literal substring term. The parser
is shared by the CLI argument, the TUI input line, and scaffold `--search`;
`--any` (CLI) and the `a` toggle (TUI) both set `Query.any`. No `OR` keyword.

### D5. Ranking and sort
Relevance score per paragraph = Σ over matched clauses of mode weight
(phrase 3, word 2, link 2, substring 1.5, fuzzy 1) × occurrence boost
(1 + 0.5 × (occ − 1), capped ×3) × position bonus (earlier first-hit ranks
higher). Tiebreak: path asc, line asc — deterministic for snapshot tests.
`--sort date`: `git blame` date desc (via the existing `BlameCache`, blamed
lazily on result-set files only — the same pattern gather uses), tie →
score, then path/line. Date lookup is skipped entirely unless the date sort
is requested. `recent` keeps the blame cache warm, so the cost stays low.

### D6. CLI surface
`ft notes search <query> [--any] [--sort relevance|date] [--limit N] [--json]`.
Text rows: `path Lstart-end  <matched clause labels> <paragraph text>`
(vault-relative path, colors off in non-TTY). JSON rows:
`{ path, line_start, line_end, body, matched: [labels], score, date? }`.
Respects `synth.exclude_prefixes` (consistent with pulse). Empty query or no
matches → empty output, exit 0.

### D7. Search TUI tab
Reuses the Tasks tab's `InlineInput`/`EditBuffer` widget for the query bar
(no new input machinery). State: input string, parsed `Query`, result rows,
selection set, `any` bool, `sort` enum, cursor. Every keypress re-parses and
re-queries the snapshot's index synchronously (the index is read-only after
build; single-threaded TUI is fine because queries are sub-ms). Status bar
renders the live parse: `N terms · AND/ANY · sort: relevance`. Keys: `a`
all↔any, `o` cycle sort, `Space` multi-select, `Enter` open source at line,
`s`/`S` send-to-synth (existing `SynthSendState` machinery extracted from the
Gather tab into `ft/src/tui/synth_send.rs` and shared), `R` re-query. Registers
a `<TAB>_COMMANDS`/`<TAB>_KEYMAP` pair, an overlay, a `help_sections()` entry,
and a `TestBackend` snapshot per the workspace conventions.

### D8. Pulse handoff
Pulse `Enter` builds `[[title1]] [[title2]]` from the selected (or cursor)
rows and raises `AppRequest::SearchWithQuery { query, any: true }` — the
same behavior multi-target gather had (OR over links), now over search.
The old `GatherForMulti` request and `GatherWindow` stay only for the hidden
Gather tab.

### D9. Scaffold sourcing
`--search "<query>"`: parse → query index → results → `SynthSource`
(dedup by `(path, line_start)` inside the result set) → existing
`plan_synth_scaffold`/`apply_synth_scaffold` unchanged (append-dedup and the
dirty-source guard live there). Sections emit in result order — relevance by
default, `--sort date` restores newest-first. `--link "[[X]]"` (transitional)
lowers to an any-mode search over the given links; single-target
Related-alias resolution is deliberately dropped from this path. `--from`
is unchanged. Scaffold requires at least one of `--search`/`--link`/`--from`.

### D10. Deprecation mechanics
- `ft notes gather` / `ft notes journal`: clap `hide = true` + one stderr
  warning on run, pointing at `ft notes search`. Behavior unchanged; the
  engine stays (used by `recent`, `related`, ghost promotion).
- Gather TUI tab: removed from `build_tabs_with_overlays` default lineup;
  restored when `[tui] show_gather = true` (new config field, same pattern as
  `tasks_tab`/`timeblocks_tab`). Search tab takes its default slot.
- Ghost promotion (`Shift+p`) re-sources via `--search "[[ghost]]"` instead
  of `build_gather`.
- `grow` and the window flags are removed outright (breaking, in the release
  notes); `filter_missing` (append dedup) stays inside the scaffold planner.

## Risks / Trade-offs

- **Substring-default latency at very large vault scale** → perf gate
  (`FT_PERF_TESTS=1`, < 10 ms per query at 5k paragraphs); trigram substring
  index is the documented fallback, deferred until measured.
- **Fuzzy recall on short terms** → terms ≤ 3 chars prefix-scan the
  dictionary instead of trigrams; threshold rules are explicit in the spec.
- **Search surfaces already-quoted material from old synth notes** → append
  dedup prevents re-pinning; `-` exclude is available; documented in the
  guide. The citation layer's removal decision is deferred to the follow-up.
- **Deprecation warnings break scripts** → warning is a single stderr line,
  never stdout; `--json` output is untouched; hidden commands still exit 0.
- **`--link` loses alias resolution during transition** → deliberate,
  documented; the search term `[[X]]` is the honest successor and `=X` covers
  word-boundary cases.
- **TUI query on a stale index** (mutation happened, snapshot not rebuilt) →
  the search tab re-derives on `on_graph_ready`/`on_focus` like every other
  tab; `R` forces a re-query after a refresh.

## Migration Plan

1. Land `ft-core::search` + `ft notes search` (purely additive).
2. Land scaffold `--search`, the Search tab, the pulse handoff, ghost
   promotion re-source (additive; old paths still work).
3. In the same change: hide gather/journal + warning, `[tui] show_gather`
   opt-in, remove `grow` + window flags (breaking; release notes).
4. Follow-up change (after the search has been used for a while): delete the
   gather engine, Gather tab, handoff request types, citation layer, and
   blame cache if it becomes dead; archive `synth-grow` and the gather specs.

Rollback: gather is never deleted in this change — un-hiding is a one-line
clap change; `[tui] show_gather = true` restores the tab; `--link`/`--from`
still source scaffolds.

## Open Questions

None blocking — the DSL, defaults (substring, relevance sort, any-mode
handoff), and scope boundaries (recent/reslice/verify/repair untouched) were
confirmed during design. Open for later: mixed AND/OR groups, folder scoping,
persisted index, and whether the citation layer dies in the removal change.
