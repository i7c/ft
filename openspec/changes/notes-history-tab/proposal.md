## Why

The synthesis ritual (`link_review` + `journal` + `synth`) surfaces paragraphs
by *link target* — "everything that mentions `[[X]]`". But a large part of
day-to-day review is time-shaped, not topic-shaped: "what did I actually write
or change in the last week, everywhere in the vault?" Today there is no surface
for that. `ft notes journal` can filter its target-scoped feed to a window
(`--in-window`), but it still requires a target and never shows an untargeted,
recency-ordered view. This change adds that view and wires it into the same
synth/move machinery so recently-edited prose can be pulled into synth notes or
relocated without leaving the workflow.

## What Changes

- New core builder `ft_core::history::build_history` alongside
  `build_journal`: iterates all vault paragraph nodes, keeps those whose lines
  were edited within a time window, and returns them ordered like the journal
  (blame `NaiveDate` descending, then source title, then `line_start`).
- New `ft notes history` CLI subcommand: the same `--since`/`--range` window
  arguments as `ft notes journal`, defaulting to `7d`, plus `--json` /
  `--no-color`. Whole-vault, paragraph-granular, reverse-chronological.
- New TUI **History** tab: renders the same paragraph-row feed, participates in
  the shared graph snapshot, and offers two row actions:
  - **Synth**: the existing select-one/many/all → protected `[!ft-source]`
    callout scaffold flow, targeting another note. No `ft-synth-target`
    frontmatter is written (there is no target), and the synth-grow/accrete
    machinery is not offered — History synth is scaffold-only.
  - **Move section** (TUI only): open the existing section-move modal seeded to
    the selected row's source note.
- Synth notes (`ft-synth: true`) are excluded from the feed by default,
  overridable with a flag; periodic/daily notes are included.
- Performance: prefilter to files touched in the window via git log, and blame
  only those files. A paragraph appears iff its own lines changed in the window.
- `ft notes journal` and its Journal tab are **unchanged** — `build_journal`,
  its arguments, and its semantics are untouched.

## Capabilities

### New Capabilities
- `notes-history`: the core `build_history` feed (whole-vault, windowed,
  recency-ordered paragraph entries with blame dates) and the `ft notes history`
  CLI surface, including the window default, synth-note exclusion, and
  file-prefilter performance contract.
- `history-tui-tab`: the TUI History tab — feed rendering, shared-snapshot
  participation, the select→synth scaffold action, and the seeded section-move
  action.

### Modified Capabilities
<!-- None: journal, synth-notes, and the section-move modal are reused without
     changing their existing requirements. -->

## Impact

- **New code**: `ft-core/src/history.rs` (builder), `ft/src/cmd/notes.rs` (new
  `History` subcommand + args + output rendering, reusing the journal renderers),
  `ft/src/tui/tabs/history.rs` (new tab), plus registry/keymap wiring
  (`command.rs`, `keymap.rs`, `build_tabs_with_overlays`, `docs/keybindings.md`).
- **Reused unchanged**: `Graph::nodes` / `ParagraphData` and owning-heading
  edges (paragraph + section data), `blame_cache` (`paragraph_date`),
  `link_review` window/added-lines resolution for the edit filter, the synth
  scaffold/callout flow, and the `SectionMoveState` modal (a thin
  `begin_for_source` entry point reusing `advance_to_multiselect`).
- **No breaking changes**; no new dependencies.
