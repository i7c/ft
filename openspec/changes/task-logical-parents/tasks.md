## 1. Resolution plumbing

- [ ] 1.1 `frontmatter::ft_tasks_anchor_in` (+ `ft_tasks_anchor`) with tests mirroring `ft_tasks_section_in`.
- [ ] 1.2 `ParsedFile::block_ids: Vec<(String, usize)>` collected in the parse pass (trailing `^id` on any non-skipped line).
- [ ] 1.3 `NoteLookup` trait in `graph/resolve.rs`; make `resolve_wiki`/`resolve_md` generic; impl for `Graph` (no behavior change) and a `ScanNoteIndex` built from `scan.files`. Existing resolve tests stay green.

## 2. Parent links in the scan

- [ ] 2.1 `ParentLink` / `ParentLinkState` types; `Task::parent_link: Option<ParentLink>` (serde skip when `None`).
- [ ] 2.2 `task::links::resolve_parent_links(&mut Scan)`: indexes (block, anchor), per-task candidate classification (block-ref, anchor-note, dangling, extras), `Ignored` when physically nested.
- [ ] 2.3 Cycle detection over effective parents; mark greatest-`TaskKey` member `Cycle`.
- [ ] 2.4 Call the post-pass at the end of `scan_vault`.
- [ ] 2.5 Tests: block-ref cross-file, same-file `[[#^id]]`, anchor note, ordinary link ignored, non-task block id ignored, dangling (missing note / missing id / anchor without 🆔), extras, ignored, 2- and 3-cycles, deleted parent.
- [ ] 2.6 Perf test (`FT_PERF_TESTS=1`) comparing scan with/without post-pass on the `realistic` fixture.

## 3. Effective hierarchy

- [ ] 3.1 `hierarchy::parent_key`; switch `parent_idx` / `children_map` to it with physical-then-linked child ordering.
- [ ] 3.2 `Graph::insert_subtask_edges` uses `parent_key` (cross-file edges); update its doc comment.
- [ ] 3.3 Tests: `expand_forest` / `expand_forest_visible` nest linked children and dedupe roots; graph has cross-file `Subtask` edge and none for dangling/ignored/cycle.

## 4. DSL + presets

- [ ] 4.1 `TaskData` fields for parent description/status, link state, child count, filled at build.
- [ ] 4.2 `Attr::{Parent, ParentStatus, ParentLink, Children}` in lexer/parser/eval with type validation (optional except `children`); reject on edges.
- [ ] 4.3 Built-in presets `orphaned`, `broken-links` + round-trip parse tests.
- [ ] 4.4 Docs: `docs/graph-query-dsl.md` / `docs/query-dsl.md` attribute tables.

## 5. Link / unlink planners

- [ ] 5.1 `link_text(from_rel, to_rel, anchor, &NoteLookup)` (same-file, unique title, path fallback, anchor-note form).
- [ ] 5.2 Block id generator with test seam; uniqueness against file `^id`s.
- [ ] 5.3 `plan_link` (+ `apply_link_plan`): ensure block id, replace/append child link, `WouldCycle`, `PhysicallyNested`, `LineChanged`/`TargetChanged`, same-file batching.
- [ ] 5.4 `plan_unlink`: remove `raw` + one space; `NoParentLink`.
- [ ] 5.5 Tests for each scenario in `task-link-cli` spec.

## 6. Move integration

- [ ] 6.1 `plan_move` strips `expected.parent_link` from head lines for `UnderTask` targets; tests (paste, indent keep other links, `--to` keeps link).
- [ ] 6.2 `plan_retarget(plan, scan, ctx)`: collect moved `^id`s, rewrite incoming links on `new`/disk content, merge edits; same-file ↔ explicit form conversions.
- [ ] 6.3 Tests: parent moved cross-file retargets journal link; link inside moved block; target file already edited; embed + alias preserved; same-file move no-op.

## 7. CLI

- [ ] 7.1 `ft tasks link <SEL>... --under <SEL>` (`--dry-run`, `--format`).
- [ ] 7.2 `ft tasks unlink <SEL>` (`--dry-run`, `--format`).
- [ ] 7.3 `ft tasks create --link-parent <SEL>` (conflicts with `--parent`; anchor-note form for anchor tasks).
- [ ] 7.4 `ft tasks check [--format json]`, exit 1 on issues.
- [ ] 7.5 `ft tasks move` calls `plan_retarget`; text output mentions retargeted link count.
- [ ] 7.6 Integration tests in `ft/tests/tasks_link.rs` (+ insta snapshots for JSON shapes) incl. `--tree` output with a linked child.

## 8. TUI

- [ ] 8.1 `CommandDef`s `tasks.link`, `tasks.unlink` (Restructure), `tasks.new-linked-subtask` (Create / edit); bind `L`, `U`, `S`; help overlay rows.
- [ ] 8.2 `tasks.link` / `tasks.unlink` handlers with toasts per spec; expand parent after link.
- [ ] 8.3 `S` quickline: pending `link_parent`, create + link on submit.
- [ ] 8.4 `DisplayRow::linked` + `↳` glyph.
- [ ] 8.5 Paste (`p`/`P`) and `M` move call `plan_retarget`; toast mentions retargeted links.
- [ ] 8.6 `TestBackend` tests: link via `d`+`L` (snapshot with `↳`), unlink, `S` create-linked, cycle toast, physically-nested toast, paste retargets an incoming link.
- [ ] 8.7 Regenerate `docs/keybindings.md`.

## 9. ft.nvim

- [ ] 9.1 [ft.nvim] Link command (pick parent task → `ft tasks link <file>:<line> --under <file>:<line> --format json`) and unlink on cursor line.
- [ ] 9.2 [ft.nvim] `ft tasks check --format json` → quickfix list.
- [ ] 9.3 [ft.nvim] Document the new CLI surface in `ARCHITECTURE.md`.

## 10. Docs + build invariants

- [ ] 10.1 `docs/guide/tasks.md`: "Linked parents" section (syntax, anchor notes, link/unlink/check, TUI keys); `docs/task-format.md`: parent-link rules.
- [ ] 10.2 `cargo build --release`
- [ ] 10.3 `cargo test --workspace`
- [ ] 10.4 `cargo clippy --workspace --tests -- -D warnings`
- [ ] 10.5 `cargo fmt --check`
- [ ] 10.6 `cargo run --release -q -- commands docs --check`
