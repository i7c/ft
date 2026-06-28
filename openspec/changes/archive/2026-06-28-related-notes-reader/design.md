## Context

`ft` already has two graph-derived reading surfaces over a note:

- **`ft notes journal`** (CLI print + full TUI Journal tab): reverse-chronological
  paragraph-mention feed, git-backed dates, multi-source, send-to-synth. Read-only.
- **`ft notes update-related`** (TUI modal only): co-occurrence scoring
  (`ft_core::related::score_related`) suggesting what belongs in a note's
  `## Related` section. Write-only by framing — the only way to *see* the
  scored candidates is to enter the commit modal.

The asymmetry: related data has no read surface, and a phantom
(unresolved `[[link]]` with no backing file) cannot be inspected at all,
because `score_related` early-returns on non-`Note` nodes and the modal
needs a file to write. Yet a phantom is arguably the concept *most* in
need of "what's related to this?" context.

The shared seam is already in place: both features call
`ft_core::journal::resolve_related_aliases` to read a note's `## Related`
section, and `score_related` reuses that alias resolution. So "related"
already means "the note plus its declared Related aliases" in both
features — semantics are consistent; only the read/write split and ghost
support are missing.

This change is intentionally small and additive: one new print command,
one core generalization (ghosts), and a reframe of the existing modal.
No new graph kinds, no new edges, no new TUI tab, no new cross-tab
`AppRequest`.

## Goals / Non-Goals

**Goals:**
- A read-only `ft notes related` command, parity with `ft notes journal`
  and `ft notes backlinks`/`links` output ergonomics, working on notes
  *and* ghosts, with no git dependency.
- `score_related` generalized to ghosts.
- The TUI Related modal reframed as a unified read+write panel (Option B),
  so opening it is no longer implicitly a write.
- Keep the read/write split clean by command name: `ft notes related`
  prints; `ft notes update-related` launches the writer.

**Non-Goals:**
- No new TUI tab for related (a flat scored list is a modal's weight, not
  a tab's; the journal earned its tab via deep per-entry paragraphs,
  synth handoff, and cross-tab entry — related data has none of that).
- No read-only ghost mode in the modal (`target_path: Option<…>`).
  Ghosts can't be written, so they stay out of the write surface; ghost
  reading is fully delivered by `ft notes related`. A follow-up can add
  in-TUI ghost reading if wanted.
- No multi-source related scoring (no `--link` repeatable form). Related
  scoring is over a single target + its Related aliases; the positional
  `<note>` may itself be a `[[Ghost]]`.
- No change to `ft notes update-related` or `ft notes journal` observable
  behavior. The `resolve_journal_target` rename is an internal refactor.

## Decisions

### D1: Print-only CLI command, not a TUI launcher
`ft notes related` only prints. The TUI surface is reached via `ft tui`
+ `R` (graph tab). This mirrors `ft notes journal` (print) vs the Journal
*tab* (entered via `ft tui` / cross-tab). Decision confirmed in
requirements: the read/write split is by command name, so `ft notes
related` (read) and `ft notes update-related` (write) are cleanly
separated. **Alternative considered:** have `ft notes related` also
optionally launch the TUI modal like `update-related`. Rejected — it
muddies the read/write split and duplicates `update-related`'s job.

### D2: Extend `score_related` to ghosts (mirror `journal::build_journal`)
The ghost branch sets `note_path = None`, alias set empty (a ghost has
no Related section / no file to read), and runs the co-occurrence walk
unchanged — ghosts can be `ParagraphLink` targets. `already_in_related`
is uniformly `false` for ghost-target results. This is a ~5-line change
in `related.rs` mirroring `journal.rs` lines 120-128. **Alternative
considered:** keep scoring note-only and special-case ghosts at the CLI.
Rejected — pushes ghost-awareness into every consumer and leaves
`score_related` lying about its own contract (the existing spec already
says "concept (note or ghost)").

### D3: Unify the modal as read+write (Option B), keep it Note-only
The modal already renders the full reader surface (`already` rows +
scored `candidates`). The only thing making it "write-only" is framing
(title "Update Related", `?`-help / `CommandDef` wording) and `Enter` =
commit. Reframe those to "Related"; keep `Enter` = commit, `Esc`/`q` =
close-without-writing (already read-safe). `R` binding unchanged
(normalizes to `Shift+r`, matching `J`→`Shift+j`; lowercase `r` = rename
is a distinct chord — verified no conflict). The modal stays Note-only:
ghosts can't be written, so they're not in the write surface.
**Alternatives considered:** (A) a new Related tab — rejected, a flat
list is modal-weight, not tab-weight, and it'd duplicate the modal's
commit flow; (C) hybrid modal+tab — rejected as overkill for this data
shape.

### D4: Shared note-or-ghost resolver, renamed
`resolve_journal_target` becomes `resolve_note_or_ghost` and is reused by
`ft notes related`. The journal's single call site is updated; behavior
is byte-identical (the `[[]]`-aware path → title → fuzzy → ghost
fallback is exactly what `ft notes related` needs). **Alternative
considered:** duplicate the resolver in `run_related`. Rejected — two
copies of the same 30-line resolution ladder drift.

### D5: Output mirrors `ft notes links`, not `ft notes journal`
`RelatedRow { title, score, already_in_related, target: LinkRowTarget }`
+ four renderers in a new `ft/src/output/related.rs`, structured like
`output/links.rs`. `LinkRowTarget` (`Resolved{path}` / `Unresolved{raw}`)
is reused so the JSON path field matches `ft notes links` exactly.
**Why not the journal's `render_*` helpers:** related rows are flat
list rows (like links), not dated paragraph blocks (like journal), so
the links shape is the right model.

## Risks / Trade-offs

- **[Modal is still write-capable] → keep framing honest.** The unified
  modal can still commit on Enter; a user opening it "just to read"
  could accidentally toggle+commit. Mitigation: `Esc`/`q` is the
  documented close-without-write, Space is an explicit opt-in per row,
  and commit requires at least one checked entry (existing guard:
  empty `selected_titles` → no write, silent close). No extra
  confirmation needed; matches the current modal's safety.
- **[Ghost scoring semantics] → alias-free.** A ghost target never has
  `already_in_related` rows, so the ghost output is "candidates only".
  This is correct (there's no Related section to read) but slightly
  different from note output (which includes marked aliases). Mitigation:
  documented in the spec scenario; not a regression.
- **[`resolve_journal_target` rename ripple] → one call site.** The
  rename touches `run_journal` plus the function def. Low blast radius.
  Mitigation: `cargo check` + the journal integration tests
  (`ft/tests/notes_journal.rs`, `journal_multi_link_cli.rs`) guard
  behavior.
- **[No snapshot asserts the modal title] → safe rename.** Verified:
  the only "Update Related" literal is `graph.rs:5004` (the title
  string); insta snapshots don't reference it; the modal tests assert
  content (`[[Alias]]`, `[[C]]`) and `Shift+r`-in-help, not the title.
  Mitigation: keep the `graph_related_modal_*` tests green.

## Migration Plan

No data migration, no config change, no breaking API. Purely additive +
reframe.

1. Land `ft-core` ghost-scoring change + unit tests first (no consumer
   change yet; `score_related` still works for notes).
2. Add `ft notes related` CLI + output module + integration tests.
3. Rename `resolve_journal_target` → `resolve_note_or_ghost`, update the
   journal call site (behavior-preserving).
4. Reframe the modal (title string, help wording, `CommandDef`
   descriptions); regenerate `docs/keybindings.md`.
5. Run all five build invariants.

Rollback: revert the commit; no on-disk artifacts to clean (the only new
file is `ft/tests/notes_related.rs` + `ft/src/output/related.rs`).

## Open Questions

None. All decisions confirmed during requirements:
- D1 print-only; D2 ghost scoring; D3 modal Option B + Note-only; D4
  shared resolver; D5 links-shaped output.
