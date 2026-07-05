## Context

The synthesis ritual surfaces paragraphs by link target. `ft notes journal` /
the Journal tab answer "what mentions `[[X]]`?", with an optional `--in-window`
overlay that filters that target-scoped feed to a git window. There is no
untargeted, time-shaped view: "what did I edit anywhere in the vault recently?"

All the pieces already exist:
- `Graph::nodes()` iterates every node; `NodeKind::Paragraph(ParagraphData)`
  carries `source_file`/`line_start`/`line_end`/`text`, and owning-heading
  structure is reachable via `OwnsParagraph`/`OwnsHeading` edges — the same data
  `build_journal` reads.
- `blame_cache::paragraph_date` yields a paragraph's most-recent-commit date.
- `link_review::compute_link_review` already computes a per-file
  `added_lines: Map<PathBuf, Set<u32>>` for a window (this is what journal's
  `--in-window` filter consumes).
- The synth scaffold/callout flow and the `SectionMoveState` modal are
  tab-agnostic and already invoked from multiple tabs.

This change is therefore mostly wiring: a new builder that selects paragraphs by
window-edit rather than by link, a thin CLI subcommand, and a new tab that reuses
the journal row rendering, the synth flow, and the section-move modal.

## Goals / Non-Goals

**Goals:**
- A `build_history` core builder that returns journal-shaped entries selected by
  window-edit, ordered exactly like the journal.
- A read-only `ft notes history` CLI surface sharing the journal's renderers.
- A TUI History tab with select→synth (scaffold-only) and a seeded section-move.
- Reuse graph paragraph/heading data, blame cache, link-review windowing, the
  synth flow, and the move modal — no re-parsing and no new movement primitive.
- Leave `build_journal` and the Journal tab byte-for-byte unchanged.

**Non-Goals:**
- No all-time (unwindowed) mode; the window is the sole scope bound (no result cap).
- No `ft-synth-target` frontmatter and no synth-grow/accrete for History synth.
- No bare-paragraph move primitive; move operates on the enclosing heading section
  via the existing modal's own heading-select step.
- No change to journal semantics or its `--in-window` behavior.

## Decisions

### Reuse `link_review` added-lines as the inclusion filter
`build_history` resolves the window, calls `compute_link_review` to obtain
`added_lines`, and includes a paragraph iff `line_start..=line_end` overlaps that
file's added-line set. This is exactly journal's `--in-window` predicate, lifted
to be the *primary* selector instead of an overlay. Rationale: it already exists,
is tested, and gives the perf prefilter for free — `added_lines`' keys are the
only files we must blame. Alternative considered: blame every file and threshold
on date — rejected as O(vault) blame cost and semantically muddier ("file touched
elsewhere" would leak in).

### Entry shape and ordering shared with the journal
Reuse the journal's entry rendering and sort. `build_history` can return the
existing `JournalEntry` shape (with `matched` empty/omitted) or a sibling
`HistoryEntry` that the journal renderers accept. Decision: return a dedicated
`HistoryEntry` (fields `date`, `source_title`, `source_path`, `line_start`,
`line_end`, `section_text`) and factor the journal's table/JSON renderers to a
shared helper the history command calls, so History's JSON does not carry a
`matched` field it never populates. Sort key is `(date desc, source_title asc,
line_start asc)`, identical to journal.

### Synth-note exclusion via frontmatter marker
Exclude paragraphs whose source note has `ft-synth: true`. `build_history` checks
the note's frontmatter (already parsed during scan / available via the vault) and
drops synth-note paragraphs unless `--include-synth`. Periodic notes are not
special-cased — they are included.

### Seeded section-move: `begin_for_source`
Add `section_move::begin_for_source(ctx, source_rel) -> SectionMoveState` that
performs the same work `handle_source_picker_key` does on a picker hit — i.e. calls
`advance_to_multiselect` for the known source path — so the modal opens at the
heading multi-select step. The History tab dispatches this into
`ActiveModal::SectionMove`. No changes to the move state machine, extraction, or
apply path.

### Tab follows the standard Tab checklist
New `ft/src/tui/tabs/history.rs` implementing `Tab` with a `TabKind`,
`HISTORY_COMMANDS` + `HISTORY_KEYMAP`, `help_sections()`, a `dispatch_command`
arm, shared-snapshot reads, `request_graph_refresh()` after the move, a session
`BlameCache`, and a `TestBackend` snapshot. Registered in
`build_tabs_with_overlays` with `.with_keymap_overlay(...)`; `docs/keybindings.md`
regenerated.

## Risks / Trade-offs

- **`compute_link_review` is currently framed around wikilinks** → It also
  produces the general `added_lines` map journal already consumes for
  `--in-window`; History uses only that map, so the link-scan portion is
  incidental. If its cost is non-trivial on large windows, a future refactor can
  split added-lines computation into a standalone helper. Mitigation: reuse
  as-is first; the window bounds the work.
- **Whole-vault paragraph iteration** → Bounded by the file prefilter (only
  window-touched files' paragraphs are considered) and by the graph already being
  built; no extra scan. Gated by the existing perf-test seam if needed.
- **Renderer refactor could perturb journal snapshots** → Extract the shared
  renderer without changing journal output; existing journal `insta` snapshots
  are the regression guard.
- **New tab widens the tab strip** → Consistent with the documented "new TUI tab"
  workflow; keymap resolves modal → tab → global, and the overlay wiring is
  covered by `ft commands check-keymap`.

## Open Questions

- Exact key bindings for the History tab's select / synth / move actions
  (resolve against the existing registry to avoid collisions during apply).
- Whether `HistoryEntry` and `JournalEntry` should ultimately converge on one
  struct; kept separate here to avoid touching journal, revisitable later.
