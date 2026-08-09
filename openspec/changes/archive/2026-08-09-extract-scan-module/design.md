# Design — extract-scan-module

## Context

Today `vault.rs` (996 LOC) owns discovery, config loading, the scan
orchestration, three walkers (`markdown_files`, `markdown_files_with_mtime`,
`directories`), the private per-file parser (`parse_file`), and the
pipeline contract types (`Scan`, `ParsedFile`). The consequences:

- **A module cycle.** `vault → crate::graph::parser` (link extraction) and
  `graph → crate::vault::Vault` (`Graph::build` needs the vault only for
  `vault.directories()`; `graph::drift` needs it for config). The
  pipeline is conceptually linear — `discover → scan → build → query` —
  but the code interleaves stage 1 (vault) with stage 2 (scan).
- **Two full read passes in the TUI hot path.** `build_graph_snapshot`
  calls `vault.scan()` (read everything) and then `CitationIndex::build`
  (walk + read everything again). The `citation-index` spec already
  requires building "from a vault scan" — the implementation drifted.
- **Three walkers with three semantics.** The vault walkers (absolute
  paths, git-ignore + `ignored_paths` + `attachments` exclusions) and
  `synth::verify::walk_markdown_files` (relative paths, dotfile-skip
  only — visits `attachments/` and gitignored files) disagree about
  what "every markdown file" means.
- **A bloated public surface.** `Scan`/`ParsedFile`/walker internals are
  exported from `vault` when they are the parse-pipeline contract, not a
  vault concern.

Constraints: the repo forbids backward-compat shims and dead-code
retention; the five build invariants must stay green after every
numbered task section; `ft` (binary) is the only consumer of `ft-core`.

## Goals / Non-Goals

**Goals:**

- One module owns the single read pass: one walk (files + dirs), one
  read per file, extracting tasks, links, headings, paragraphs, and the
  frontmatter block.
- Break the `vault ⇄ graph` cycle: `Graph::build` takes only a `&Scan`;
  `graph/mod.rs` has zero production imports of `crate::vault`.
- Kill the second full read pass in the TUI hot path (citations built
  from the scan).
- Converge the three walkers on one implementation and one exclusion
  policy.
- Zero churn at the 235 `.scan()` call sites and at every CLI surface.
- Add the missing direct unit tests for the walkers and the parse
  pipeline.

**Non-Goals:**

- Full file content in the snapshot (paragraph texts already carry the
  body; storing raw content again duplicates memory — see decision 5).
- Migrating `search`, `recent`, `pulse`, `gather` off their targeted
  reads — they operate without a scan in scope (follow-up: pass the
  TUI snapshot).
- `Task`/`TaskData` model dedup, `drift`/`ghosts` promotion,
  `refresh_note` adoption — separate findings from the same review.
- Any CLI or output change.

## Decisions

### 1. Module: `ft-core/src/scan.rs`, entry point `scan::scan_vault(root, ignored)`

The module owns the walk and the parse; `Scan` / `ParsedFile` /
`ScanError` / `DEFAULT_IGNORED` live here. `ignored_paths` is passed as
data (`&[String]`); the module does not read config.

- *Alternative — a `parse` module:* the module is broader than parsing
  (walk + directory collection + error aggregation), so `scan` is the
  honest name.
- *Alternative — keep in `vault.rs`, just split methods:* leaves the
  cycle and the misplacement intact; rejected.

### 2. `Scan` gains `dirs`; one walker pass yields files + dirs

One `WalkBuilder` pass collects both the markdown-file list (absolute
during the walk, relativized for output) and the directory list.
`directories()` as a public API disappears — `Scan::dirs` is the
interface. The defensive parent-directory union in `Graph::build`
(`insert_directory_nodes`) stays, now computed against `scan.dirs`.

- *Alternative — keep two walks:* doubles walk cost and lets the two
  sets drift; rejected. This also fixes the current inconsistency where
  `markdown_files` and `directories` duplicate the override-building
  logic.

### 3. `Graph::build(scan: &Scan)` — vault parameter dropped

Directory nodes come from `scan.dirs`. `graph/mod.rs` drops
`use crate::vault::{ParsedFile, Scan, Vault}` entirely (production
code); `graph::drift` keeps its own vault dependency (separate finding,
out of scope). The ~232 call sites (≈220 in tests) are updated
mechanically — mostly `Graph::build(&v, &scan)` → `Graph::build(&scan)`.
No compat wrapper (repo convention).

### 4. `Vault::scan()` kept as a one-line delegator

`Vault::scan() -> Scan` keeps its exact signature and delegates to
`scan::scan_vault(&self.path, &self.config.config.ignored_paths)`. All
235 call sites are untouched. Vault's three walker methods are deleted
(the only external walker consumers are `search.rs`'s two sites, which
move to `scan::` directly).

### 5. Frontmatter as the raw block: `ParsedFile::frontmatter: Option<String>` + block-taking readers

The scan captures the raw text between the YAML fences (what
`frontmatter::block` returns today as a private helper). `frontmatter.rs`
gains `pub fn block(content: &str) -> Option<&str>` and block-taking
variants of the four readers; the content-taking readers delegate to
block + `_in`. This was the user-confirmed decision.

- *Alternative — resolved keys at scan time:* eager parse of four keys
  per file when most files use none; less composable. Rejected.
- *Alternative — full raw content in `ParsedFile`:* duplicates what
  `ParagraphData::text` already holds, inflates the snapshot, and blurs
  the contract (the snapshot is parse artifacts, not a vault mirror).
  Rejected; mutation-time callers must keep reading live files anyway
  (they edit them).

### 6. `CitationIndex::build(root: &Path, scan: &Scan)`

Synth-note discovery iterates `scan.files` and reads
`ParsedFile::frontmatter` (zero disk reads); only synth-marked notes
get their content read (once) for callout parsing. This enforces the
existing `citation-index` spec wording ("buildable from a vault scan")
and removes the TUI's second full read pass. Callers: `tui/snapshot.rs`
and `cmd/notes.rs` (×2), which already hold a scan.

### 7. Walker convergence: `walk_markdown_files` deleted

`verify::verify_all`, `repair::plan_repair_all`, and the citation build
switch to `scan::markdown_files`. The deliberate behavior change —
synth sweeps now use the scanner's exclusions (hidden, git-ignored,
`DEFAULT_IGNORED` folders, config `ignored_paths`) — makes "every
markdown file" mean the same thing everywhere. Fixture-driven synth
tests are checked for reliance on the old dotfile-only universe.

### 8. `scan::markdown_files` returns vault-relative paths

The only consumer (`search.rs::fuzzy_find`) relativizes immediately
(`rel(&p, &vault.path)`); returning relative makes that a no-op and
matches `walk_markdown_files`'s convention that verify/citations/repair
already assume.

### 9. `ScanError` moves to `scan.rs`

Its only references are `vault.rs` (4 sites) and `Scan::errors`; the
crate error module keeps `Error`/`Result`. `Scan` keeps its `Default`
derive (now with the `dirs` field).

## Risks / Trade-offs

- **[Test ripple]** ≈220 `Graph::build` test call sites change → the
  change is mechanical (arg drop) and each task section keeps the five
  invariants green; the sweep task is a codemod + `cargo test
  --workspace` verification.
- **[Synth sweep behavior change]** verify/repair/citations stop
  visiting `attachments/` and gitignored files → deliberate (one file
  universe everywhere); flagged in the proposal and exercised by the
  fixture tests.
- **[Path semantics change]** `markdown_files` absolute → relative →
  contained to `search.rs`; `rel()` becomes identity; search tests are
  the guard.
- **[Frontmatter capture cost]** one extra block scan per file →
  `frontmatter::block` is the same string scan the readers already do;
  the captured block is a single small string per frontmatter-bearing
  file.
- **[Behavioral drift on citations]** index contents must be identical
  before/after (same synth notes, same callouts) → covered by existing
  citation tests plus the snapshot lifecycle tests.

## Migration Plan

Each numbered section leaves the workspace compiling and the five
invariants green (the repo's stepwise convention, same as
shared-graph-snapshot):

1. `frontmatter.rs`: expose `block()` (no behavior change).
2. Create `scan.rs`: move types, walkers, `parse_file`; single-walk
   core; frontmatter capture; `ScanError` move; `lib.rs`.
3. `Vault::scan()` delegates; walker methods deleted; `search.rs` moved
   to `scan::`.
4. `Graph::build(scan)` + `cmd/common.rs` + snapshot builder + the
   ~220 test call sites.
5. Frontmatter `_in` readers (content readers delegate).
6. Walker convergence in synth + `CitationIndex::build(root, scan)` +
   callers.
7. New unit tests (walkers, parse_file frontmatter capture, `_in`
   readers, scan→graph equality guard).
8. Full invariant pass; grep for leftover `vault::{Scan,ParsedFile}`
   imports and any `Graph::build` vault-arg signatures; update
   `docs/architecture.md` references to the moved types.

Rollback: no schema or on-disk format changes — revert the commits; the
archive note records the paired commit shas.

## Open Questions

- None blocking. Minor: whether `cmd/notes.rs`'s two citation call
  sites reuse their existing scan or build one — resolved in tasks by
  reusing the scan already in scope.
