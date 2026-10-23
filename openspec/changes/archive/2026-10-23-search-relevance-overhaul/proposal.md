## Why

Paragraph search (`ft notes search`, the Search tab, `synth scaffold
--search`) is the sourcing front-end of the synthesis workflow, but its
ranking is an ad-hoc sum of per-clause weights with no document
frequency or length normalization, so long bullet lists that repeat a
term outrank focused notes. A paragraph is matched and ranked without its
note's title, aliases, heading path, or tags. The tokenizer is ASCII-only,
which silently breaks word and fuzzy clauses on accented text: `=ônibus`
returns zero results on a real vault while `=nibus` returns six. There
are no scoping filters, and results are hard to scan (whole paragraphs
flattened to one line, no highlighting, no grouping by note). Measured on
a 2,047-note vault, a `memory` query ranks a 30-line project bullet list
above the note whose heading is about shared memory.

The fix is a proper lexical ranking model plus the graph and git signals
that only `ft` has. It is cheap: no new heavy dependencies, no model
downloads, and the in-memory, per-keystroke architecture stays. It is
also the foundation a later meaning-based (embedding) layer would build
on.

## What Changes

- **Relevance evaluation harness first.** A committed fixture vault plus
  graded `query → paragraph` judgments, scored with nDCG@10 and MRR in
  `cargo test`, so every ranking change is measured. An optional
  external judgments file runs the same harness against a real vault.
- **BM25F ranking.** Replaces the per-clause weight sum: BM25 term
  weighting with per-field length normalization over the paragraph body
  plus per-note context fields (note title, frontmatter aliases,
  enclosing heading path, tags). **BREAKING** (ranking only): result
  order changes for most queries. Result sets change only where
  context-field matching applies (next bullet).
- **Context-field matching.** A positive clause matches a paragraph when
  the term appears in its body *or* in its context fields, so
  `eigen memoization` finds the paragraph about eigen inside the
  `Memoization` note. Exclude clauses stay body-only.
- **Unicode-aware tokenization and folding.** Word tokens are runs of
  Unicode alphanumerics. Index and query text are case-folded and
  diacritic-folded (`busão` ≡ `busao`). Optional Snowball stemming for
  `=word` clauses, set by a `[search] stemmers` config list (default
  `["english"]`).
- **In-query filters.** `path:<prefix>`, `tag:<tag>`,
  `after:<date>` / `before:<date>` (paragraph blame date, same date
  syntax as the graph DSL), and `scope:<graph-preset>`, which restricts
  results to notes selected by a graph preset and so reuses the graph DSL
  rather than inventing a fourth query language. All filters can be
  negated with `-`. Unknown `key:` prefixes stay literal text, so parsing
  never fails.
- **Graph- and git-aware soft signals.** A bounded multiplicative prior
  from note in-degree (backlinks) and file recency, plus a
  `near:[[Note]]` clause that boosts paragraphs by link-graph distance
  from a seed note. Signals reorder results and never filter them.
- **Readable results.**
  - Each result carries its note title, heading path, highlight ranges,
    and per-clause match reasons (`body`, `title`, `heading`, `alias`,
    `tag`).
  - CLI text output groups results by note, with a highlighted snippet
    window per paragraph; `--flat` restores one row per paragraph.
  - `--json` keeps every existing key and adds `title`, `heading_path`,
    `highlights`, and `reasons`.
  - The TUI shows the heading path per row, highlights matches in the
    preview, and toggles grouped/flat with `g`.
- The 10 ms / 5,000-paragraph performance budget is kept.

## Non-goals

- **Meaning-based (embedding) search, re-ranking, and query expansion.**
  A follow-up change, layered on this one through Reciprocal Rank Fusion.
- **Flagging or hiding already-synthesized paragraphs.** It needs a
  citation lookup, and `CitationIndex` was deliberately removed by
  `remove-pulse-and-recent`. Re-introducing it is a separate decision.
  Scaffold's append-dedup already prevents duplicate pins.
- **A persistent on-disk index.** Not needed until embeddings exist.
- **Changing the file/heading fuzzy finder** (`ft notes find`, picker).

## Capabilities

### New Capabilities
- `search-relevance-eval`: the relevance evaluation harness (judgment
  file format, fixture vault, nDCG@10 / MRR metrics, ratchet thresholds
  in `cargo test`, and an optional external real-vault run).

### Modified Capabilities
- `paragraph-search`: the index gains Unicode tokenization, folding,
  context fields, term frequencies, and optional graph priors. The
  grammar gains filter clauses and `near:`. Matching extends to context
  fields and stem equivalence. Ranking becomes BM25F with bounded priors.
  CLI output gains grouping, snippets, highlights, `--flat`, and extra
  JSON keys. The TUI tab gains a heading path, highlights, and the `g`
  grouping toggle. The performance budget is restated for the new index.

## Impact

- **ft-core:**
  - `search/{index,engine,query}.rs` (rewritten internals; the public
    `SearchIndex::build`, `parse_query`, `search`, `search_with_dates`
    entry points stay, with extended result and option types);
  - a new `search/text.rs` (tokenizer, folding, stemming);
  - `frontmatter.rs` (`aliases_in`, `tags_in` readers);
  - `config.rs` (`[search]` table).
- **ft:**
  - `cmd/search.rs` (grouped/snippet output, `--flat`, extra JSON);
  - `cmd/synth.rs` (`scaffold --search` passes the graph for parity);
  - `tui/snapshot.rs` (index built with graph signals);
  - `tui/tabs/search.rs` (heading path, highlights, `g` toggle,
    command/keymap rows, regenerated `docs/keybindings.md`).
- **Dependencies:** `unicode-normalization` (diacritic folding) and
  `rust-stemmers` (Snowball), both small and pure Rust.
- **CLI cost:** `ft notes search` now also builds the link graph (about
  0.2 s extra on a 2k-note vault), so CLI and TUI rank identically.
- **ft.nvim:** none. It does not call `ft notes search`; the JSON
  changes are additive.
- **Docs:** `docs/architecture.md` §Synthesis, README search section,
  user guide query reference.
