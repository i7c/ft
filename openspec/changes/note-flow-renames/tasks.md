# Tasks: note-flow-renames

## 1. Core renames (compile-driven)

- [ ] 1.1 Rename `ft_core::link_review` → `pulse` (`compute_pulse`,
  `Pulse`, `PulseRow`; `WindowRange` stays); fix all consumers rustc
  lists
- [ ] 1.2 Rename `ft_core::journal` → `gather` (`GatherEntry`,
  `GatherReport`, `build_gather`); fix consumers
- [ ] 1.3 Rename `ft_core::history` → `recent` (`RecentEntry`,
  `RecentReport`, `build_recent`, `RecentOptions`); fix consumers
- [ ] 1.4 `cargo test -p ft-core` green (snapshot churn reviewed, not
  blind-accepted)

## 2. CLI surface

- [ ] 2.1 `ft notes journal` → `ft notes gather`, `ft notes history` →
  `ft notes recent` (variants, args structs, help text)
- [ ] 2.2 Move `ft review` → `ft notes pulse` and `ft synth <sub>` →
  `ft notes synth <sub>`; delete the top-level commands from
  `Commands` in `main.rs`; no aliases
- [ ] 2.3 Sweep "ritual" from all CLI help strings and command docs
- [ ] 2.4 Update CLI integration tests (renamed commands, snapshots) —
  including citation_badges, notes_ghosts, notes_drift fixtures that
  shell out to renamed commands; suite green

## 3. TUI: IDs, titles, order, config gate

- [ ] 3.1 Rename tab modules/types (`tabs/journal`→`gather`,
  `tabs/history`→`recent`, `tabs/review`→`pulse`; `TabKind`,
  `JournalTarget`→`GatherTarget`, `AppRequest::Journal*`→`Gather*`),
  command IDs (`journal.*`→`gather.*`, `recent.*`, `pulse.*`) and
  keymaps (chords unchanged)
- [ ] 3.2 Reorder `build_tabs_with_overlays` to Graph, Notes, Pulse,
  Recent, Gather, Tasks, Timeblocks; update tab-index assumptions in
  tests/helpers
- [ ] 3.3 Add `[tui]` config table (`tasks_tab`, `timeblocks_tab`,
  default false, unknown keys rejected) and gate the two tabs in
  `build_tabs_with_overlays`; verify tab digits/cycling derive from
  the built list
- [ ] 3.4 Verify `ft do` behavior for tasks/timeblocks commands with
  tabs disabled (dispatchable, or a clear error) and cover with a test
- [ ] 3.5 Batch-review the TUI snapshot churn (titles/order/IDs only),
  regenerate `docs/keybindings.md`, `commands docs --check` green
- [ ] 3.6 New tests: default five-tab layout snapshot; enabled-tabs
  layout; `[tui]` unknown-key rejection

## 4. Docs + specs sweep

- [ ] 4.1 README: rewrite demo with new commands, re-verify output
  against a scratch vault; update guide pages (notes.md, synthesis.md,
  index.md chapter table, graph.md, philosophy.md, scripting/commands
  examples) to new names; add the `[tui]` re-enable note
- [ ] 4.2 Dev docs: CLAUDE.md, AGENTS.md, docs/architecture.md
  (rename the "Synthesis ritual" section; fix inbound references),
  docs/commands.md examples
- [ ] 4.3 Mechanical wording pass over live `openspec/specs/*`
  (old command names + ritual); archived changes untouched
- [ ] 4.4 Final grep gate: `rg -i ritual` (allowlist: archive,
  explorations) and `rg` for old command spellings return nothing
  unintended

## 5. Wrap-up

- [ ] 5.1 Update `openspec/explorations/note-flow-reframe.md`
  session 5 status + decisions; note the parking lot as the surviving
  backlog
- [ ] 5.2 Full invariant sweep: `cargo build --release`,
  `cargo test --workspace`, `cargo clippy --workspace --tests -- -D
  warnings`, `cargo fmt --check`, `commands docs --check`
