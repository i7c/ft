# Tasks — extract-scan-module

Keep all five build invariants green after every numbered section
(`cargo build --release`, `cargo test --workspace`,
`cargo clippy --workspace --tests -- -D warnings`, `cargo fmt --check`,
`ft commands docs --check`). Order follows design.md §Migration Plan;
each section leaves the workspace compiling and tested.

## 1. Frontmatter block seam (prerequisite, no behavior change)

- [ ] 1.1 Make `frontmatter::block(content) -> Option<&str>` public
      (rename the private `frontmatter_block`), with a doc comment
      stating it returns the raw text between the YAML fences.
      Expected: existing frontmatter tests pass unchanged; no behavior
      change.

## 2. Scaffold the scan module

- [ ] 2.1 Create `ft-core/src/scan.rs` and register `pub mod scan;` in
      `lib.rs`. Move `DEFAULT_IGNORED`, `Scan`, `ParsedFile`, and
      `ScanError` (from `error.rs`) here. `Scan` gains
      `dirs: Vec<PathBuf>`; `ParsedFile` gains
      `frontmatter: Option<String>`; both keep `Debug` and `Scan` keeps
      `Default`.
- [ ] 2.2 Move `parse_file` (from `vault.rs`) into `scan.rs` as a
      private function; have it capture `frontmatter::block(&content)`
      into `ParsedFile::frontmatter` in the same read.
- [ ] 2.3 Move the three walkers as free functions:
      `scan::markdown_files`, `scan::markdown_files_with_mtime`
      (both now returning vault-relative paths), and a private
      single-walk core that yields both the file list and the
      directory list from one `WalkBuilder` pass. `Scan::dirs` is
      populated from that walk. Delete the duplicated
      override-building blocks (one shared helper).
      Expected: scan module compiles standalone; `scan_vault(root,
      ignored)` produces files, dirs, tasks, frontmatter, and errors.
- [ ] 2.4 Unit tests in `scan.rs`: default exclusions
      (`.obsidian/`, `.git/`, `attachments/`), config `ignored_paths`,
      hidden/git-ignored files, one-walk consistency (a file in an
      excluded dir implies the dir is absent from `dirs`), and
      relative-path output for `markdown_files`. Expected: the
      previously untested walkers now have direct coverage.

## 3. Vault delegation

- [ ] 3.1 `Vault::scan()` body becomes
      `scan::scan_vault(&self.path, &self.config.config.ignored_paths)`
      with the signature unchanged. Delete the `markdown_files`,
      `markdown_files_with_mtime`, and `directories` methods from
      `Vault`, and the `crate::graph::parser` + `task::emoji` imports
      they implied (only `scan` remains). Expected: all 235 `.scan()`
      call sites compile untouched; `Vault` no longer imports graph
      or task-format internals.
- [ ] 3.2 Update `search.rs`'s two call sites to
      `scan::markdown_files(&vault.path, &ignored)` /
      `scan::markdown_files_with_mtime(...)`; simplify the
      now-identity `rel()` in `fuzzy_find` if it becomes unused.
      Expected: all search tests pass unchanged.

## 4. Graph decoupling

- [ ] 4.1 `Graph::build(scan: &Scan)` — drop the `&Vault` parameter;
      directory nodes come from `scan.dirs` (keep the defensive parent
      union in `insert_directory_nodes`). Remove the
      `use crate::vault::{ParsedFile, Scan, Vault}` import from
      `graph/mod.rs`. Expected: `graph/mod.rs` has zero production
      imports of `crate::vault` (grep-verifiable).
- [ ] 4.2 `cmd/common.rs::build_graph(scan)` drops the vault param;
      update the 10 `cmd/{tasks,pulse,graph,synth,notes}.rs` call
      sites. Expected: binary compiles; CLI integration tests pass.
- [ ] 4.3 Update `ft/src/tui/snapshot.rs::build_graph_snapshot` body to
      `Graph::build(&scan)` (signature unchanged). Expected: TUI
      snapshot lifecycle tests pass unchanged.
- [ ] 4.4 Sweep the ~220 test call sites of
      `Graph::build(&vault, &scan)` / `Graph::build(&v, &v.scan())`
      → `Graph::build(&scan)` across `graph/tests.rs`,
      `graph/query/tests.rs`, `graph/{rename,resolve,ghosts,drift,
      preset}.rs`, `pulse.rs`, `gather.rs`, `recent.rs`, `related.rs`,
      `ft-core/tests/{graph_query_matrix,real_vault}.rs`,
      `ft/src/tui/tabs/graph/tests.rs`, `ft/src/tui/widgets/picker.rs`,
      `ft/src/tui/tests/*`; update `crate::vault::{Scan,..}` imports to
      `crate::scan::{Scan,..}`. Codemod (sed for the uniform patterns),
      then `cargo test --workspace` until green. Expected: workspace
      tests green; graph snapshot equality is implicitly guarded by the
      existing graph tests (same artifacts → same graph).

## 5. Frontmatter block readers

- [ ] 5.1 Add `ft_tasks_section_in`, `ft_append_section_in`,
      `ft_synth_enabled_in`, `ft_synth_targets_in` (block-taking);
      rewrite the content-taking readers to delegate
      (`block(content)?` + `_in`). Expected: existing frontmatter
      tests pass; new `_in` unit tests verify equivalence with the
      content readers.

## 6. Walker convergence + citation index

- [ ] 6.1 Replace `synth::verify::walk_markdown_files` call sites
      (`verify.rs`, `citations.rs`, `repair.rs`) with
      `scan::markdown_files(&vault.path, &vault.config.config.ignored_paths)`
      and delete `walk_markdown_files`. Expected: synth verify/repair
      sweep the scanner's file universe; update any fixture-driven
      synth tests whose fixtures relied on the old dotfile-only
      exclusion (e.g. a synth note under `attachments/`).
- [ ] 6.2 `CitationIndex::build(root: &Path, scan: &Scan)` — synth
      discovery from `ParsedFile::frontmatter` (zero reads); read only
      synth-marked notes' content for callout parsing. Update callers:
      `tui/snapshot.rs`, `cmd/notes.rs` (×2, reusing the scan already
      in scope). Expected: citation tests pass; `build_graph_snapshot`
      performs exactly one read pass (grep-verifiable: no
      `read_to_string` in the non-synth path); index contents are
      unchanged for existing fixtures.

## 7. New coverage

- [ ] 7.1 `scan.rs` parse tests: frontmatter capture present/absent;
      task lines inside fenced code or YAML frontmatter are not parsed
      (the `LineSkipState` invariant now unit-tested directly at the
      scan level).
- [ ] 7.2 Scan→graph equality guard: for the `tiny/` and `realistic/`
      fixture vaults, assert the graph built from a fresh scan equals
      the graph built before this change (node/edge counts, directory
      nodes from `dirs`, `HasTask`/`OwnsTask` totals). Expected: a
      regression net over the dirs-fold and the rel-path change.
- [ ] 7.3 Grep-audit task: `rg 'vault::(Scan|ParsedFile)'` and
      `rg 'Graph::build\(&v` return nothing; `rg 'walk_markdown_files'`
      returns nothing; `rg '\.markdown_files\(|\.directories\('`
      returns only scan-module internals. Expected: zero leftovers.
- [ ] 7.4 Update `docs/architecture.md` references to the moved types
      (`vault::Scan`/`ParsedFile` → `scan::`, `Graph::build` signature,
      citation build from the scan).

## 8. Final verification

- [ ] 8.1 Run all five build invariants; fix any fallout.
- [ ] 8.2 Confirm no CLI behavior change: run the `ft/tests/*` CLI
      integration suite (drives the binary) — all pass.
- [ ] 8.3 Confirm the TUI hot path reads once: in
      `build_graph_snapshot`, `vault.scan()` is the only full vault
      read (citation build is scan-fed). Expected: verified by code
      review + the citation tests.
