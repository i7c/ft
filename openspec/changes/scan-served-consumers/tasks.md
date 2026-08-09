# Tasks — scan-served-consumers

Keep all five build invariants green after every numbered section
(`cargo build --release`, `cargo test --workspace`,
`cargo clippy --workspace --tests -- -D warnings`, `cargo fmt --check`,
`ft commands docs --check`). Order follows design.md §Migration Plan.

## 1. Scan metadata

- [ ] 1.1 Add `mtime: std::time::SystemTime` and
      `line_count: u32` to `ParsedFile`. Set `.require_metadata(true)`
      on the walker; `walk()` yields mtimes alongside file paths;
      `parse_file` computes `line_count` from the content it reads.
      Expected: `scan_vault` populates both fields; no second
      filesystem pass.
- [ ] 1.2 Delete `scan::markdown_files_with_mtime` (no remaining
      callers after task 2). Expected: `Scan::files` is the only mtime
      source.
- [ ] 1.3 Scan tests: distinct-mtime capture (write two files with
      forced distinct mtimes, assert `ParsedFile::mtime` ordering) and
      line-count correctness (multi-line file, empty file).

## 2. recent_hits consumes the scan

- [ ] 2.1 `search::recent_hits(scan: &Scan, recents: &RecentsLog,
      limit) -> Vec<Hit>` — file set and mtimes from `scan.files`.
      Update the two TUI picker call sites and search tests.
      Expected: `recent_hits` does no vault walk; empty-input picker
      list behavior unchanged.

## 3. History feed served from the scan

- [ ] 3.1 `recent::build_recent(..., scan: &Scan)` gains the scan
      parameter. Replace the synth-marker memo's content read with
      `frontmatter::ft_synth_enabled_in(pf.frontmatter)` when the file
      is in `scan.files`; fall back to the current read when absent.
      Drop the `synth::callout::is_synth_note` import.
      Expected: non-synth files are never read for the marker; the
      `recent → synth` edge is gone; output identical for files present
      in the scan.
- [ ] 3.2 Update `build_recent` callers (recent tab, cmd/notes.rs
      `run_recent`, ft-core tests) with a scan argument.
- [ ] 3.3 Tests: (a) fixture with a synth note — excluded without a
      marker read; (b) a file absent from the scan (created after
      `scan()`) falls back to the direct read and is still excluded.

## 4. Link review served from the scan

- [ ] 4.1 `pulse::compute_pulse(..., scan: &Scan)` gains the scan
      parameter. The synth gate for callout exclusion uses the scan's
      frontmatter when the file is in `scan.files` (staleness accepted
      per design); synth-marked files still read content (working tree
      or `git show`) for callout ranges; files absent from the scan
      fall back to the current read+check. Drop the
      `synth::callout::is_synth_note` import (callout parsing stays).
      Expected: non-synth touched files are not read; the
      `pulse → synth` marker edge is gone; link-review output unchanged
      for scan-present files.
- [ ] 4.2 Update `compute_pulse` callers (pulse tab, gather tab's
      in-window path, cmd/pulse.rs, cmd/synth.rs, cmd/notes.rs, ft-core
      tests) with a scan argument — bind `let scan = vault.scan()` in
      cmd/pulse.rs and cmd/synth.rs where missing.
- [ ] 4.3 Tests: (a) synth-note callout exclusion works with a scan-fed
      gate; (b) fallback for a file created after the scan.

## 5. Journal aliases from the scan

- [ ] 5.1 `gather::find_related_range(headings, total_lines: u32)` —
      drop the content parameter (callers pass the line count).
- [ ] 5.2 `gather::resolve_related_aliases(graph, note_id, pf:
      &ParsedFile)` — headings + line count from the parsed file;
      fall back to the current content read when the target has no
      `ParsedFile` (ghost targets, or file absent from the scan).
- [ ] 5.3 `gather::build_gather(..., scan: &Scan)` gains the scan
      parameter; look up the single-mode target's `ParsedFile` by path.
      Update callers (gather tab, cmd/notes.rs `run_gather`, ft-core
      tests).
      Expected: journal single-target alias resolution reads no content
      when the target is in the scan; multi-target and ghost behavior
      unchanged.

## 6. Scan-fed TUI picker

- [ ] 6.1 `search::fuzzy_find_from_scan(scan: &Scan, query, opts) ->
      Vec<Hit>` — same scoring as `fuzzy_find` with stage-1 paths and
      stage-2 headings from `Scan::files` (share the scoring internals
      rather than duplicating). Expected: byte-identical ranking to
      `fuzzy_find` for the same vault state (equivalence test).
- [ ] 6.2 TUI picker: `FilePickerSource` (and the empty-input source)
      hold `Option<Arc<Scan>>`; `query()` uses
      `fuzzy_find_from_scan` / `recent_hits(scan, …)` when a scan is
      present, else the vault-based path. The modal construction sites
      pass `ctx.snapshot`'s scan.
      Expected: per-keystroke filtering with a snapshot installed does
      zero vault I/O; pre-snapshot fallback keeps the picker usable.
- [ ] 6.3 Picker tests: scan-fed query matches vault-fed results for a
      fixed fixture; fallback path exercised without a snapshot.

## 7. CLI + TUI call-site sweep

- [ ] 7.1 Bind and pass scans in `cmd/pulse.rs`, `cmd/synth.rs`,
      `cmd/notes.rs` (run_gather already binds one; run_recent,
      run_gather now pass it to the new signatures). Expected: CLI
      output byte-identical to before (fresh per-invocation scan).
- [ ] 7.2 TUI tabs pass `&snapshot.scan` at
      `tabs/pulse.rs`, `tabs/recent.rs`, `tabs/gather.rs` call sites.
      Expected: TUI snapshot tests pass unchanged.

## 8. Audits + coverage

- [ ] 8.1 Layering audit: `rg 'is_synth_note' ft-core/src/recent.rs
      ft-core/src/pulse.rs` returns nothing (marker checks go through
      `frontmatter`); `rg 'markdown_files_with_mtime'` returns nothing.
- [ ] 8.2 Read-path audit: `read_to_string` in recent.rs/pulse.rs
      appears only in the fallback or synth-file paths.
- [ ] 8.3 Staleness test: build a scan, then create a synth note;
      assert the TUI-style scan-fed path treats it as non-synth until a
      re-scan, while the fallback path (file absent) still excludes it.
- [ ] 8.4 Equivalence test: `fuzzy_find_from_scan(scan, q, opts)` vs
      `fuzzy_find(vault, q, opts)` over the `tiny/` fixture produce the
      same hits.

## 9. Final verification

- [ ] 9.1 Run all five build invariants; fix fallout.
- [ ] 9.2 Grep-audit: no `build_recent`/`compute_pulse`/`build_gather`/
      `recent_hits` call site passes a vault where a scan is now
      required; `ft find` code path untouched (no scan build added).
