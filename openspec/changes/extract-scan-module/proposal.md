# Extract scan module (one read pass extracts everything)

## Why

The scan/parse pipeline — the walk, the per-file parse, and the contract
types `Scan` / `ParsedFile` — is buried inside `vault.rs`, which creates
the only real module cycle in `ft-core` (`vault → graph::parser` for link
extraction; `graph → vault` because `Graph::build` needs `Vault` only for
its directory walk). The TUI hot path pays twice: `build_graph_snapshot`
runs `vault.scan()` **and** `CitationIndex::build(vault)`, a second full
walk + read of every file. And a third walker implementation
(`synth::verify::walk_markdown_files`) with different exclusion
semantics exists for the synth sweeps. This is finding 1 of the
2026-08-08 core architecture review.

## What Changes

- New `ft-core/src/scan.rs` module owning the single read pass: **one
  walk** (markdown files + directories from the same walker pass) and
  **one read per file** extracting tasks, raw links, headings,
  paragraphs, and the raw `ft:` frontmatter block.
- `Scan`, `ParsedFile`, and `ScanError` move out of `vault.rs` /
  `error.rs` into `scan.rs`. `Scan` gains `dirs: Vec<PathBuf>`;
  `ParsedFile` gains `frontmatter: Option<String>` (raw block between
  the YAML fences — the decision recorded in design.md §5).
- `Vault::scan()` keeps its exact signature and becomes a one-line
  delegator to `scan::scan_vault(&self.path, &self.config.config.ignored_paths)`
  — all 235 existing `.scan()` call sites are unchanged. Vault's
  walker methods (`markdown_files`, `markdown_files_with_mtime`,
  `directories`) are deleted.
- `Graph::build(scan: &Scan)` no longer takes `&Vault` — directory
  nodes come from `Scan::dirs`. **BREAKING** internal API: ~232 call
  sites updated mechanically (no compat shim, per repo convention).
- `frontmatter.rs` exposes `block(content)` and block-taking variants
  of the four readers (`ft_tasks_section_in`, `ft_append_section_in`,
  `ft_synth_enabled_in`, `ft_synth_targets_in`); the content-taking
  readers delegate. Consumers holding a `ParsedFile` resolve frontmatter
  keys without re-reading the file.
- `CitationIndex::build(root, scan)` consumes the scan: synth-note
  discovery comes from `ParsedFile::frontmatter` (zero reads); only
  synth-marked notes get their content read (once) for callout parsing.
  **BREAKING** internal API (3 call sites). This also makes the
  implementation match the existing `citation-index` spec, which
  already requires building "from a vault scan".
- The synth sweeps (`verify_all`, `plan_repair_all`, citation build)
  converge on `scan::markdown_files`; `walk_markdown_files` is deleted.
  **Behavior change:** synth sweeps now use the scanner's exclusions
  (hidden, git-ignored, `DEFAULT_IGNORED` = `.obsidian`/`.git`/
  `attachments`, config `ignored_paths`) instead of the old
  dotfile-only skip — they sweep the same universe the scanner sees.
- `scan::markdown_files` returns vault-relative paths (was absolute);
  the only consumer (`search.rs`) relativizes anyway.

## Capabilities

### New Capabilities

- `vault-scan`: the single-read-pass scan module — one walk + one read
  per file, the `Scan` snapshot contract (`tasks`, `files`, `dirs`,
  `errors`), per-file `ParsedFile` artifacts including the frontmatter
  block, walker semantics (exclusions, relative paths), read-only
  consumption by `Graph` and `Vault`, and the block-based frontmatter
  reader seam.

### Modified Capabilities

- `citation-index`: `CitationIndex::build` is modified to *require*
  construction from a vault scan (synth discovery via the scan's
  frontmatter capture; no re-walk; only synth notes read) — enforcing
  what the existing requirement already promises.

## Impact

- **`ft-core/src/scan.rs`** — new module; `vault.rs` (discovery +
  delegation only), `error.rs` (`ScanError` removed), `graph/mod.rs`
  (`build(scan)`, vault import dropped), `frontmatter.rs` (additive),
  `search.rs` (2 call sites), `synth/citations.rs`,
  `synth/verify.rs`, `synth/repair.rs` (walker swap), `lib.rs`.
- **`ft/src/cmd/common.rs`** — `build_graph(scan)`; 10 `cmd/*` call
  sites drop the vault arg. **`ft/src/tui/snapshot.rs`** — citation
  build from the same scan (kills the second read pass in the TUI hot
  path); `build_graph_snapshot` signature unchanged.
- **Tests** — ~220 `Graph::build(&vault, &scan)` test call sites
  (mechanical arg drop, mostly in `graph/tests.rs`,
  `graph/query/tests.rs`, `graph/{rename,resolve,ghosts,drift,preset}`,
  `pulse`, `gather`, `recent`, `related`, TUI tests, picker,
  `ft-core/tests/{graph_query_matrix,real_vault}.rs`); new unit tests
  for the previously untested walkers and the frontmatter capture.
  CLI integration tests (`ft/tests/*`) are unaffected (they drive the
  binary).
- **No CLI surface changes** — every `ft` subcommand keeps its exact
  arguments and output.
- **Out of scope (later changes):** `search`'s own per-file heading
  reads, `recent`/`pulse`/`gather` on-demand synth-marker reads,
  `Task`/`TaskData` model dedup, promoting `drift`/`ghosts` out of the
  graph module, and adopting `Graph::refresh_note` as an incremental
  rebuild path.
