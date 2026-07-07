# Design: note-flow-renames

## Context

The finale of the note-flow reframe. All naming decisions were made
with the user (2026-07-06): gather/recent/pulse; everything under
`ft notes`; ratify the session-2–4 names; tab order with Tasks/
Timeblocks opt-in (default off); no `ft jot`; break compatibility
freely — except vault file formats.

## Goals / Non-Goals

**Goals:**

- One coherent pass: after this change, no surface (CLI, TUI, docs,
  dev docs, specs) uses journal/history/review or "ritual" for the
  workflow, and no internal name contradicts the surface.
- The `ft notes` namespace reads as the workflow: capture (`create`,
  `append`, `today`) → resurface (`gather`, `recent`, `pulse`,
  `related`, `ghosts`, `drift`) → consolidate (`synth`, `move-section`,
  `rename`).
- The TUI opens on the note flow; adjacent features are opt-in.

**Non-Goals:**

- No aliases or deprecation shims (repo convention + user decision).
- No on-disk vault format changes: `ft-synth: true`,
  `ft-synth-targets`, `[!ft-source]`, blame cache, `[synth]` config
  table, task/timeblock formats all stay byte-identical.
- No renaming of openspec capability directories (historical planning
  artifacts); only their misleading wording is corrected.
- No new features beyond the `[tui]` config gate.

## Decisions

**D1 — the name map.** CLI: `notes journal`→`notes gather`,
`notes history`→`notes recent`, `review`→`notes pulse`,
`synth <sub>`→`notes synth <sub>`. TUI tabs: Journal→Gather,
History→Recent, Review→Pulse; `TabKind` variants follow. Command IDs:
`journal.*`→`gather.*`, `history.*`→`recent.*`, `review.*`→`pulse.*`
(verb parts unchanged, e.g. `gather.toggle-uncited`). Chords
unchanged. Ratified names keep: `ghosts`, `drift`, `related`,
`cited`/`cited*`/`--uncited`, `o`, `Shift+p`.

**D2 — internal names follow, bounded by the vault-format wall.**
Core modules rename with their types: `journal.rs`→`gather.rs`
(`GatherEntry`, `GatherReport`, `build_gather`,
`resolve_related_aliases` unchanged), `history.rs`→`recent.rs`
(`RecentEntry`, `build_recent`, `HistoryOptions`→`RecentOptions`),
`link_review.rs`→`pulse.rs` (`compute_pulse`, `Pulse`, `PulseRow`,
`WindowRange` unchanged). TUI mirrors: `tabs/journal.rs`→`gather.rs`
etc., `JournalTarget`→`GatherTarget`, `JournalWindow`→`GatherWindow`,
`AppRequest::JournalFor`→`GatherFor` and siblings. The `synth` module
and its vocabulary ("synth note", `[synth]` config) are *names of the
artifact format*, not workflow verbs — they stay. Anything serialized
into vaults stays (D-non-goal). Rationale: the repo rejects dead
names, and "gather is powered by build_journal" is a permanent
vocabulary split otherwise.

**D3 — tab order and the config gate.** Order:
**Graph, Notes, Pulse, Recent, Gather, Tasks, Timeblocks** — browse
surfaces first, then resurface in sweep→pull order (pulse: what's been
on my mind → recent: what did I touch → gather: everything about X,
where consolidation keys live), adjacent features last. New config:

```toml
[tui]
tasks_tab = false       # default
timeblocks_tab = false  # default
```

`build_tabs_with_overlays` (which already receives the config) skips
disabled tabs; tab digits and `Tab`-cycling derive from the built
list, so no special-casing. `ft do` / headless dispatch for tasks
commands is unaffected (registry membership is not tab presence —
verify this holds; if dispatch requires a live tab, disabled tabs
reject with a clear error). Unknown-key rejection gives typos in
`[tui]` immediate surfacing for free.

**D4 — the ritual sweep, final scope.** CLI help strings
(`main.rs`, `cmd/*`), code comments and module docs (`synth/mod.rs`
"Synthesis-ritual support", `config.rs`, `tests/synthesis.rs` header,
TUI tab docs), `docs/architecture.md` §"Synthesis ritual" heading
(renamed; inbound links updated), CLAUDE.md + AGENTS.md, and a
mechanical wording pass over `openspec/specs/*` (ritual +
old command names). Archived changes stay untouched (history).

**D5 — docs re-verification.** README's demo currently shows
`ft review` / `ft notes journal` / `ft synth scaffold`; it reruns
against a scratch vault with the new commands so output stays real
(session-1 discipline). Guide pages, docs/commands.md examples,
keybindings regen. The chapter table row for synthesis and the
philosophy's command mentions update in the same pass.

**D6 — snapshot churn is accepted in one batch.** Every TUI
`TestBackend` snapshot shows the tab bar, so all of them change with
the retitle/reorder; CLI snapshots referencing command names change
too. They are reviewed as a batch *after* the rename compiles and the
suite's assertion logic passes — content diffs should be exactly
titles/order/names, and anything else is a regression to chase.

## Risks / Trade-offs

- [Sheer mechanical surface] The rename touches most of the tree. →
  Grouped tasks, compile-driven: rename core first, let rustc list
  every consumer; run the suite between groups.
- [Missed stragglers] Old names hiding in strings/docs. → Final gate:
  `rg -i` for `journal`, `history`, `\breview\b`, `ritual` with an
  explicit allowlist (vault-format tokens, git-blame domain uses of
  "history", archived openspec dirs, this change's own record).
- [Users' first `ft tui` after upgrade] Tasks/Timeblocks gone. →
  Release-note-style line in README/config docs showing the two-line
  `[tui]` enable.
- [`ft do` on disabled tabs] Headless dispatch may assume tab
  presence. → Verified in tasks; explicit error if not dispatchable.

## Migration Plan

Single hard cut, one commit series in one change. Order: core renames
→ CLI surface → TUI (IDs, titles, order, config gate) → snapshots →
docs/specs sweep → final grep gate.

## Open Questions

- None — all decisions taken with the user. (Interpretation on
  record: "everything under ft notes" includes `synth` as
  `ft notes synth <sub>`.)
