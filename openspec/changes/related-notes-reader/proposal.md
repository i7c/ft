## Why

Related notes today can only be consumed *inside* the interactive
"Update Related" modal — i.e. always with write intent. The journal
already has a clean read-only surface (`ft notes journal` prints, plus a
full TUI tab); the related-notes data has no equivalent. A user who just
wants to *see* which concepts co-occur with a note has to enter the
write modal, and a phantom (unresolved link with no backing file) cannot
be inspected at all because the modal requires a Note node and a file to
write to. This change closes that asymmetry with a print-only `ft notes
related` command and unifies the existing TUI modal as a read+write
surface, so "related" stops being gated behind "update".

## What Changes

- **New `ft notes related` subcommand (read-only).** Prints the
  `score_related` output — the same scored concept list the modal
  shows, today invisible to the CLI — as `table | json | ndjson |
  markdown` (default table), with `--no-color` and `--allow-empty`.
  Mirrors `ft notes backlinks`/`links` ergonomics. No git dependency:
  scoring is pure graph, so it works on non-git vaults (unlike
  `ft notes journal`).
- **Extend `score_related` to ghosts.** A phantom target (`[[Foo]]`
  with no `Foo.md`) is exactly the case most in need of related-context
  suggestions, yet `score_related` currently bails on non-`Note` nodes.
  Ghost handling mirrors `journal::build_journal`: ghost →
  `note_path = None`, alias set empty (a ghost has no Related section to
  read), the co-occurrence walk runs unchanged. This makes both the CLI
  command and any future ghost reading correct.
- **CLI note/ghost resolver shared with journal.** The `--link` /
  positional `[[]]`-aware resolver currently named `resolve_journal_target`
  becomes the note-or-ghost resolver for both `ft notes related` and
  `ft notes journal` (renamed/generalized, no behavior change to
  journal).
- **Unify the TUI Related modal as read+write (Option B).** The modal
  already renders the full reader surface (`already`-in-Related rows +
  scored `candidates`). It stops being framed purely as a writer: the
  title "Update Related: N" → "Related: N", and the `?`-help / command
  metadata wording drops "updater"/"update" in favor of the unified
  "Related" framing. The `R` binding stays (already normalizes to
  `Shift+r`, matching `Shift+J` for the journal); `Enter` still commits
  (the write path), `Esc`/`q` still closes without writing. **Modal
  stays Note-only on the graph tab** — ghosts cannot be written, so
  ghost reading is delivered by `ft notes related` (print). No new tab,
  no cross-tab `AppRequest`, no `target_path: Option<…>` change.
- **`ft notes update-related` unchanged.** Stays the TUI-launching
  write command (notes-only). The read/write split is now by command
  name: `ft notes related` = read+print, `ft notes update-related` =
  launch writer.

## Capabilities

### New Capabilities
- `related-notes-reader`: The `ft notes related` print-only subcommand —
  graph-based co-occurrence suggestions surfaced to the CLI, parallel
  to `ft notes journal` but without git/blame. Output formats
  (`table | json | ndjson | markdown`), `--no-color`, `--allow-empty`,
  note-or-ghost target resolution.

### Modified Capabilities
- `related-updater`: `score_related` extended to accept `NodeKind::Ghost`
  targets (phantom scoring); the graph-tab Related modal reframed from
  a write-only "updater" to a unified read+write "Related" panel (title,
  help text, `CommandDef` descriptions) — behavior (toggle/commit/cancel)
  unchanged.

  (The `resolve_journal_target` → `resolve_note_or_ghost` rename is an
  implementation refactor shared by `ft notes related` and `ft notes
  journal`; the journal's observable resolution behavior is unchanged, so
  `notes-journal` is NOT a modified capability — see Impact.)

## Impact

- **`ft-core/src/related.rs`**: `score_related` ghost branch (mirror of
  `journal::build_journal`'s `NodeKind::Ghost(_) => None` path); new
  ghost-scoring unit tests.
- **`ft/src/cmd/notes.rs`**: new `NotesCommand::Related(RelatedArgs)` +
  `run_related`; generalized resolver (rename `resolve_journal_target`
  → `resolve_note_or_ghost`, update the one journal call site); no
  changes to `run_update_related` or `run_journal` bodies.
- **`ft/src/output/`**: new `related.rs` (`RelatedRow` + four renderers,
  structured like `links.rs`); register module in `output/mod.rs`.
- **`ft/src/tui/tabs/graph.rs`**: `RelatedModal` title string; one
  `HelpSection` entry wording; `graph.related` `CommandDef`
  description. No structural/flow changes.
- **`ft/src/tui/modal_commands.rs`**: `related.*` `CommandDef`
  description wording (no new commands, no keymap changes).
- **`docs/keybindings.md`**: regenerated via
  `ft commands docs` (build invariant) — description text only.
- **Tests**: new `ft/tests/notes_related.rs` (note + ghost target,
  table + json, `--allow-empty`); extend `ft-core/src/related.rs` unit
  tests for ghost scoring; the existing `graph_related_modal_*` tests
  assert content + `Shift+r` in help, not the old title string →
  survive the reframe.
- All five build invariants stay green (`build --release`, `test
  --workspace`, `clippy --tests -D warnings`, `fmt --check`,
  `commands docs --check`).
