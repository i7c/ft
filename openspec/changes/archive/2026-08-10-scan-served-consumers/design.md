# Design — scan-served-consumers

## Context

The `extract-scan-module` change (archived 2026-08-09) made `Vault::scan()`
the single read pass: one walk (files + dirs) and one read per file,
capturing tasks, links, headings, paragraphs, and the raw `ft:`
frontmatter block into `Scan` / `ParsedFile`. It also exposed
`frontmatter::block()` + the `ft_*_in(block)` readers, so any consumer
holding a `ParsedFile` can resolve frontmatter keys without re-reading.

Consumers that were left reading the filesystem:

- `recent::build_recent` reads each touched file to check
  `is_synth_note` (a per-file memo) — the `recent → synth` edge.
- `pulse::compute_pulse` reads each touched file (working tree, or
  `git show` at the ref) to check `is_synth_note` and parse callout
  ranges — the `pulse → synth` edge. Its read is bounded by the git
  window, but the marker check is per touched file.
- `gather::resolve_related_aliases` reads the target note once to
  extract headings + the `## Related` range.
- `search::recent_hits` walks the vault for mtimes.
- The TUI file/heading picker calls `fuzzy_find(vault, …)` **per
  keystroke**; heading queries read every surviving candidate file on
  every keystroke.

Decisions recorded from review: pulse staleness is acceptable (same
eventuality class as the TUI snapshot); the scan gains mtime so all
mtime consumers migrate; CLI `ft find` is deliberately not refactored
(it is a pure filesystem walk; forcing a scan would buy parse cost for
no read savings — filename-only queries read nothing today).

Constraints: no shims (repo convention); every numbered task section
keeps the five build invariants green; the TUI concurrency model is
unchanged (snapshot-fed consumers read the shared `Arc<GraphSnapshot>`).

## Goals / Non-Goals

**Goals:**

- Serve every parse-derived consumer from the scan: markers (recent,
  pulse), headings + line count (gather, picker), mtimes (recent_hits).
- Kill the `recent → synth` and `pulse → synth` feature couplings
  (marker checks move to `frontmatter`).
- TUI picker filtering becomes zero-I/O per keystroke (in-memory scan
  data instead of a walk + candidate reads).
- CLI behavior identical: every `ft` command keeps fresh semantics via
  a per-invocation scan; `ft find` untouched.
- One mtime source: the scan. Delete `markdown_files_with_mtime`.

**Non-Goals:**

- Refactoring CLI `ft find`.
- Full file content in the snapshot (only the metadata + artifacts the
  consumers need).
- `Task`/`TaskData` dedup, `drift`/`ghosts` promotion, `refresh_note`
  adoption.
- Any CLI output change.

## Decisions

### 1. `ParsedFile` gains `mtime` + `line_count`; walker uses `require_metadata`

```rust
pub struct ParsedFile {
    pub rel: PathBuf,
    pub links: Vec<RawLink>,
    pub paragraphs: Vec<Paragraph>,
    pub headings: Vec<Heading>,
    pub frontmatter: Option<String>,
    pub mtime: std::time::SystemTime,  // NEW — from the same walk
    pub line_count: u32,               // NEW — from the content read
}
```

The walker sets `.require_metadata(true)`, so `entry.metadata()` is
served by the walk itself (cheap on platforms where readdir carries
stat data; a single stat where it does not — not a second walk).
`parse_file` computes `line_count` from the content it already holds.
`scan::markdown_files_with_mtime` is deleted; `Scan::files` is the only
mtime source.

- *Alternative — keep `markdown_files_with_mtime` and add mtime to
  Scan:* two mtime sources that can disagree (walk-time vs scan-time);
  rejected. One source, captured in the single pass.
- *Alternative — store mtimes on `Scan` as a side table:* the mtime
  belongs to the file artifact; on `ParsedFile` is where consumers
  (picker rows, recent_hits) look.

### 2. Consumer signatures gain `&Scan`; fallback read when a file is absent

```rust
pub fn build_recent(graph, vault, window, cfg, opts, cache, scan: &Scan) -> Result<RecentReport>;
pub fn compute_pulse(graph, vault, window, cfg, scan: &Scan) -> Result<Pulse>;
pub fn build_gather(graph, targets, vault, cache, scan: &Scan) -> Result<GatherReport>;
pub fn recent_hits(scan: &Scan, recents: &RecentsLog, limit: usize) -> Vec<Hit>;
```

Each consumer looks up the file by `rel` in `scan.files`; when absent
(read error during the scan, or file created after the scan), it falls
back to the current direct read. The lookup is a linear find over
`scan.files` (the touched/target set is small in every consumer).

- **recent**: the gate is `frontmatter::ft_synth_enabled_in(pf.frontmatter)`
  — a boolean, zero reads. Files not in the scan fall back to the
  content read (the existing memo path).
- **pulse**: the gate uses the scan's frontmatter (staleness accepted,
  user decision). Synth-marked files still read content (working tree
  or `git show`) for callout ranges; non-synth files are never read.
  Files not in the scan fall back to the current read+check.
- **gather**: `resolve_related_aliases(graph, note_id, pf)` uses
  `pf.headings` + `pf.line_count`; `find_related_range(headings,
  total_lines)` drops the content parameter. Ghost targets have no
  `ParsedFile` → no aliases (same as today's read-error path). When the
  target's `ParsedFile` is absent, fall back to the current read.

### 3. `search::fuzzy_find_from_scan` for the TUI picker; `fuzzy_find(vault, …)` stays

`fuzzy_find_from_scan(scan, query, opts)` reuses the exact scoring
pipeline of `fuzzy_find` with stage-1 paths from `Scan::files` (rel)
and stage-2 headings from `ParsedFile::headings` — zero I/O. The CLI
keeps `fuzzy_find(vault, …)` (user decision: no parse cost for a
filesystem walk).

The picker sources (`FilePickerSource` in `tui/widgets/picker.rs`)
hold `Option<Arc<Scan>>`:
- `Some(scan)` → `fuzzy_find_from_scan` / `recent_hits(scan, …)` —
  the steady state, per-keystroke cheap.
- `None` (before the first snapshot lands) → the current vault-based
  path. After the snapshot installs, the modal re-opens the picker with
  the scan (the picker is rebuilt per open, so the swap is at
  construction time).

The modals that construct the picker already receive `TabCtx` with
`snapshot`; they pass `ctx.snapshot.map(|s| Arc::clone(&s.scan))`.

### 4. TUI call sites pass `&snapshot.scan`

The three tab call sites (`tabs/pulse.rs:175`, `tabs/recent.rs:275`,
`tabs/gather.rs:461` and its second `compute_pulse` at `:554`) already
read `graph` from `ctx.snapshot`; they pass `&snapshot.scan` alongside.
The tabs only compute when a snapshot exists (they render a loading
state otherwise), so there is no pre-snapshot case in the tabs — only
in the picker (decision 3).

### 5. CLI paths bind a fresh scan and pass it

`cmd/pulse.rs`, `cmd/synth.rs` bind `let scan = vault.scan()` (fresh
per invocation — CLI behavior unchanged); `cmd/notes.rs`'s
`run_gather`/`run_recent` already bind one (from the previous change)
and pass it to `build_gather`/`build_recent`/`compute_pulse`.

## Risks / Trade-offs

- **[Eventual consistency in the TUI]** A note whose marker/headings
  change after the snapshot was built is not visible to the feeds,
  picker, or journal until the next graph rebuild → explicitly
  accepted (pulse) and consistent with the shared-graph-snapshot
  staleness envelope; the CLI stays fresh. Documented in the
  vault-scan spec.
- **[Fallback complexity]** Dual paths (scan-fed + direct-read) in
  three consumers → the fallback is narrow (file absent from scan) and
  reuses the existing code path; covered by dedicated tests.
- **[Test churn]** ft-core recent/pulse/gather tests gain scan
  parameters (~40 call sites) → mechanical (their helpers already call
  `vault.scan()`); each task section keeps the invariants green.
- **[mtime on non-unix]** mtime granularity differs across platforms →
  existing `recent_hits` tests already normalize (UNIX_EPOCH fallback,
  distinct-mtime ordering with sleeps); the scan's mtime uses the same
  `modified()` source.

## Migration Plan

Each numbered section leaves the workspace compiling and the five
invariants green:

1. Scan metadata (`mtime`, `line_count`, `require_metadata`) + tests.
2. `recent_hits(scan, …)`; delete `markdown_files_with_mtime`; search
   tests.
3. `build_recent` scan param + frontmatter gate + fallback; tests.
4. `compute_pulse` scan param + frontmatter gate + fallback; tests.
5. gather: `find_related_range(headings, total_lines)` +
   `resolve_related_aliases` from `ParsedFile` + `build_gather` scan
   param; tests.
6. `fuzzy_find_from_scan` + picker plumbing (`Option<Arc<Scan>>`);
   tests.
7. CLI + TUI call-site sweep (cmd/pulse, cmd/synth, cmd/notes, tabs).
8. Layering audit (no `synth::callout::is_synth_note` in recent/pulse)
   + grep audits (read_to_string only in fallback/synth paths) + new
   staleness/equivalence coverage.
9. Five-invariant final pass.

Rollback: no on-disk or CLI changes; revert the commits.

## Open Questions

- None blocking. Minor: whether `fuzzy_find_from_scan` should share the
  stage-2 scoring code with `fuzzy_find` via an internal generic or a
  small duplication — resolved in implementation toward sharing (the
  two pipelines differ only in the heading source).
