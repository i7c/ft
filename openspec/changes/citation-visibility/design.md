# Design: citation-visibility

## Context

`accrete::filter_missing` already answers "is this exact paragraph
pinned in this note?" by `(source_path, exact body)` with a blake3
hash-prefix fast reject; scaffold/grow use it silently at plan time.
`ft synth grow` is the note-centric accretion flow; the watermark
(`--new-only`) is a per-note time cutoff. What's missing is a *global*
index and *visible* state on the feeds (CLI and TUI), plus a live
note-local view when working toward a specific note. Decisions taken
with the user (2026-07-06): dismiss is deferred; staleness is a
first-class third state.

## Goals / Non-Goals

**Goals:**

- One index, three states: cited / cited-stale / uncited, defined once
  in ft-core and consumed identically by CLI and TUI.
- Feeds become incremental: an uncited filter turns a discovery
  session into "attend to the unbadged".
- The note-context view the user asked for: cited-in-*this*-note vs
  missing, live in the TUI.

**Non-Goals:**

- No dismiss/"seen, irrelevant" state (deferred; would be the only
  durable non-derivative state in the system).
- No git-blame lineage tracking for citations across edits.
- No persistent citation cache — computed per invocation / per
  snapshot rebuild; `.ft/cache/` is the fallback if it ever measures
  slow.
- No changes to `build_journal` / `build_history` signatures (the
  signature-ripple cost flagged in CLAUDE.md).
- No renames of existing commands or flags (session 5).

## Decisions

**D1 — identity is content, not location.** *Cited* = some synth-note
callout pins the same vault-relative source path and byte-identical
body (`compute_section_hash` prefix as fast reject, exact compare to
confirm — precisely `filter_missing`'s rule, kept consistent so the
badge never disagrees with scaffold/grow dedup). *Cited-stale* = no
exact match, but a callout pins the same source path with a line range
overlapping the paragraph's current range and a different body. The
ranges come from different revisions, so overlap is a heuristic — fine
for an advisory badge; the exact state is authoritative, the stale
state is a hint. Alternative considered: blame-lineage matching —
rejected for v1 (cost/complexity, and repair/reslice already exist for
callout maintenance).

**D2 — one ft-core module, two build points.** New
`ft_core::synth::citations` with `CitationIndex::build(...)` (iterate
synth notes from the scan, `callout::parse` each, index by
`(source_path, hash_prefix)` plus a per-path interval list for stale
lookup) and
`lookup(source_path, line_range, body) -> CitationState` returning the
citing note paths. CLI builds it inside `journal`/`history` command
runs. TUI: the background graph-rebuild worker builds it alongside the
graph and it travels in the shared `GraphSnapshot` (an
`Arc<CitationIndex>` member or adjacent field with the same
generation), so tabs read it via `TabCtx::snapshot` and never rescan.

**D3 — annotate at the edge, not in the builders.** `JournalEntry` /
history entries stay as-is; renderers (CLI output paths, TUI row
builders) call `lookup` per entry. This keeps `build_journal` /
`build_history` untouched and the index optional for existing callers
and tests.

**D4 — flag and filter semantics.** `--uncited` keeps entries whose
state is not `Cited` — stale counts as uncited for filtering, because
the triage question is "does anything still need my attention?" and an
edited-since-cited paragraph does. Text badge: `cited: <note stem>`
for exact, `cited*: <note stem>` for stale; multiple citing notes
render the first plus `+N`. JSON: `cited_in: [{note, stale}]`
(empty array when uncited) — additive, so existing consumers keep
working.

**D5 — TUI toggle mirrors existing tab-filter precedent.** New
commands `journal.toggle-uncited` / `history.toggle-uncited` bound to
`u` (following the `w` in-window toggle pattern on the Journal tab),
declared in the tabs' `*_COMMANDS`/`*_KEYMAP` statics; regenerate
`docs/keybindings.md`.

**D6 — note-context mode is a badge re-scope, not a new flow.** The
existing flows already put a target note in play: the `s`
append-to-existing picker on Journal/History, and `synth grow`'s
frontmatter-targets path. When a target is set, row badges recompute
against that note only (`in note` / `missing`; same matching rule),
so the user sees live exactly what plan-time dedup would drop. Leaving
the flow reverts to global badges. No new modal; this is row-render
state on the tab plus a status-line indicator.

**D7 — dismiss stays out, on record.** The deferral and its trigger
condition ("uncited feeds still haystack-shaped after badges land")
are recorded in the exploration doc's session-2 section and the
parking lot.

## Risks / Trade-offs

- [Hash-prefix collision] 6-hex prefix ≈ 24 bits — collisions
  plausible vault-wide. → Mitigation: prefix is only the reject
  filter; exact body compare confirms, same as `filter_missing`.
- [Stale false positives] Line ranges from different revisions can
  overlap coincidentally after heavy edits. → Accepted: stale is
  advisory, filtering treats it as uncited, and the badge names the
  note so the user can check.
- [Snapshot size/time] Callout parsing joins the graph-rebuild
  worker. Synth notes are few (dozens); parse is one regex pass per
  note. → Measure in the perf-gated tests; `.ft/cache/` if ever
  needed.
- [TUI snapshot churn] Row badges change existing `TestBackend`
  snapshots for Journal/History tabs. → Budgeted in tasks; review
  snapshot diffs rather than blind-accepting.
- [Multi-note citations] A paragraph cited in three notes could
  clutter rows. → First note + `+N`, full list in JSON and (TUI)
  the entry detail view if one exists.

## Migration Plan

Purely additive; no stored format changes. Ship CLI and core first
(tasks are ordered so `--uncited`/badges work headlessly before the
TUI wiring), TUI second.

## Open Questions

- Exact `GraphSnapshot` integration: member field vs. sibling
  `Arc` slot filled by the same worker — decide at implementation
  against the current `App` structure (both satisfy D2; prefer
  whichever keeps `pump_graph_rebuild_for_test` untouched).
