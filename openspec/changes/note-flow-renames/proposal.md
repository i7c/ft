# Proposal: note-flow-renames

## Why

Sessions 1–4 settled the conceptual frame (capture → resurface →
consolidate; pull vs. sweep) and shipped the features, deliberately
deferring names. The current surface still lies about itself: a
"journal" you never write into, a "history"/"journal" pair whose help
text disambiguates by analogy, a "review" that collides with PKM
weekly reviews and still says "Synthesis ritual" in its `--help`, and
a one-workflow family scattered across three namespaces. Session 5
(exploration record) is the single coherent naming pass, decided with
the user on 2026-07-06.

## What Changes

All **BREAKING** — the user explicitly waived backwards compatibility
(no aliases, no shims; scripts and `[keymap]` overrides must update):

- **Core renames (decided):** `ft notes journal` → `ft notes gather`;
  `ft notes history` → `ft notes recent`; `ft review` →
  `ft notes pulse`.
- **Namespace consolidation (decided):** everything workflow-shaped
  moves under `ft notes` — `ft review` → `ft notes pulse` and
  `ft synth <sub>` → `ft notes synth <sub>` (scaffold, grow, verify,
  repair, reslice). Top-level `review` and `synth` are removed.
- **TUI follows:** tabs retitle (Journal → Gather, History → Recent,
  Review → Pulse); command IDs rename (`journal.*` → `gather.*`,
  `history.*` → `recent.*`, `review.*` → `pulse.*`); tab order becomes
  **Graph, Notes, Pulse, Recent, Gather, Tasks, Timeblocks** —
  resurface order sweep→pull, adjacent features last.
- **Tasks/Timeblocks tabs become opt-in:** new `[tui]` config table
  with `tasks_tab` / `timeblocks_tab` booleans, **default `false`** —
  fresh setups see the five note-flow tabs; task/time users enable
  theirs. **BREAKING** for existing TUI users (one config line to
  restore).
- **Internal names follow the surface** (repo convention: no dead
  names): core modules `journal` → `gather`, `history` → `recent`,
  `link_review` → `pulse`, with types/functions renamed accordingly
  (`JournalEntry` → `GatherEntry`, `build_journal` → `build_gather`,
  `compute_link_review` → `compute_pulse`, `JournalTarget` →
  `GatherTarget`, …).
- **"ritual" sweep completes:** CLI help strings, code comments,
  docs/architecture.md's section heading, CLAUDE.md, AGENTS.md, and
  openspec/specs wording (mechanical pass; capability directory names
  stay).
- **Docs re-verified:** README demo and guide pages rewritten to the
  new commands with output re-verified against a real vault;
  `docs/keybindings.md` regenerated.
- **Ratified as-is (no change):** `ft notes ghosts`, `ft notes drift`,
  `cited`/`cited*`/`--uncited`, `o` (open-for-synth-note),
  `Shift+p` (promote-ghost). **No `ft jot`** — `ft notes append` and
  TUI quick capture cover capture.
- **Explicitly untouched (vault data compatibility):** on-disk formats
  — `ft-synth: true`, `ft-synth-targets`, `[!ft-source]` callouts,
  `.ft/cache/blame.msgpack`, the `[synth]` config table, and all task/
  timeblock formats. Breaking scripts is fine; breaking vaults is not.

## Capabilities

### New Capabilities

- `note-flow-naming`: the naming contract — the `ft notes` namespace
  as the workflow home, the gather/recent/pulse verbs, the tab order
  and opt-in adjacent tabs, and the no-ritual wording rule extended to
  every surface.

### Modified Capabilities

<!-- Existing capability specs (notes-journal, notes-history,
journal-tui-tab, history-tui-tab, link-review, synth-notes,
synthesis-review-tui-tab, …) keep their directory names as historical
planning artifacts; their requirement texts get a mechanical
command-name/wording pass recorded as a task, not per-capability
delta files — the naming contract itself lives in the new capability. -->

## Impact

- Widest change of the five sessions. `ft` CLI (`main.rs` dispatch,
  `cmd/notes.rs`, `cmd/review.rs` + `cmd/synth.rs` fold-in), core
  module/type renames rippling through every consumer and test, TUI
  tab registry/titles/order + config gate in `build_tabs_with_overlays`,
  command registry + keymaps (regenerate `docs/keybindings.md`).
- **Every TUI snapshot changes** (the tab bar appears in each frame) —
  the churn is the point; review as a batch after the reorder lands.
- README + guide docs command references; `ft do` examples in
  docs/commands.md; CLAUDE.md and AGENTS.md.
- User-visible breakage: shell scripts calling renamed commands,
  `[keymap]` overrides referencing old command IDs, muscle memory on
  tab digits, and Tasks/Timeblocks tabs hidden until enabled.
