# Scan-served consumers (finish the one-read-pass)

## Why

`extract-scan-module` built the infrastructure — `ParsedFile` captures
frontmatter + headings, the walker is unified, `Vault::scan()` is the
single read pass — but left the vault-reading consumers still
re-reading: the history feed and link-review read individual files to
check the synth-note marker (`recent → synth`, `pulse → synth`
feature couplings remain), the journal reads the target note for its
Related aliases, the empty-input picker list walks for mtimes, and the
TUI file/heading picker does a full vault walk + candidate reads **on
every keystroke** of a heading query. The one-read-pass motivation has
a tail: consumers that derive from parse artifacts should be served
from the scan, not from the filesystem.

## What Changes

- **Scan metadata.** `ParsedFile` gains `mtime: SystemTime` and
  `line_count: u32`. The walker obtains mtimes via the walker's
  metadata support (no second filesystem pass); `parse_file` computes
  the line count from the content it already read. `scan::markdown_files_with_mtime`
  is deleted — the scan is now the only mtime source.
- **History feed (`recent::build_recent`)** gains a `&Scan` parameter.
  The synth-note exclusion gate is served from
  `ParsedFile::frontmatter` (`ft_synth_enabled_in`) — zero reads for
  non-synth files; the `recent → synth` import is dropped.
- **Link review (`pulse::compute_pulse`)** gains a `&Scan` parameter.
  The synth-note gate for callout exclusion is served from the scan's
  frontmatter (negative filter); files the scan marks as synth notes
  are still read for their callout ranges. Staleness of the scan's
  marker vs the live file is accepted (decision: same eventuality class
  as the TUI snapshot). The `pulse → synth` `is_synth_note` import is
  dropped (callout parsing stays).
- **Journal (`gather::build_gather`)** gains a `&Scan` parameter.
  `resolve_related_aliases` reads headings + line count from the
  target's `ParsedFile` instead of re-reading the note;
  `find_related_range(headings, content)` becomes
  `find_related_range(headings, total_lines)`.
- **Empty-input picker list (`search::recent_hits`)** consumes the scan
  (file set + mtimes) instead of walking the vault.
- **TUI file/heading picker.** New `search::fuzzy_find_from_scan(scan,
  query, opts)` — same scoring, but paths and headings come from
  `Scan::files` (zero I/O per keystroke). The picker sources hold an
  `Option<Arc<Scan>>` fed from `ctx.snapshot`, falling back to the
  existing `fuzzy_find(vault, …)` before the first snapshot lands.
  **BREAKING** internal signatures: `build_recent`, `compute_pulse`,
  `build_gather`, `recent_hits` gain scan parameters.
- **CLI `ft find` is explicitly NOT touched** — it stays a filesystem
  walk with no parse cost (decision: don't buy the extra execution
  cost). `fuzzy_find(vault, …)` remains for it.

## Capabilities

### New Capabilities

<!-- none — the behavior this change adds (file metadata in the scan;
     consumers served from the scan with fallback + eventual
     consistency) is an extension of the existing vault-scan contract. -->

### Modified Capabilities

- `vault-scan`: two ADDED requirements — file metadata captured at scan
  time (mtime, line count), and vault-reading consumers served from the
  scan (markers, headings, mtimes; read fallback for files absent from
  the scan; eventually-consistent semantics in the TUI, fresh semantics
  in the CLI).

## Impact

- **`ft-core/src/scan.rs`** — `ParsedFile` fields, walker metadata,
  `markdown_files_with_mtime` removed.
- **`ft-core/src/search.rs`** — `recent_hits` takes `&Scan`;
  `fuzzy_find_from_scan` added; `fuzzy_find(vault, …)` unchanged.
- **`ft-core/src/recent.rs` / `pulse.rs` / `gather.rs`** — scan
  parameters; frontmatter/heading/line-count reads from `ParsedFile`;
  fallback reads for files absent from the scan; `recent`/`pulse` drop
  their `synth` marker import.
- **`ft/src/tui/widgets/picker.rs`** — picker sources gain the scan
  (snapshot-fed with vault fallback). **`ft/src/tui/tabs/{recent,pulse,gather}.rs`**
  — pass `&snapshot.scan` at the three `build_recent` /
  `compute_pulse` / `build_gather` call sites.
- **`ft/src/cmd/{pulse,synth,notes}.rs`** — bind a scan and pass it
  (fresh per invocation, so CLI behavior is unchanged).
- **Tests** — ft-core tests for recent/pulse/gather gain scan
  arguments (mechanical, mostly via existing `vault.scan()` helpers);
  new coverage for mtime/line-count capture, the fallback paths, scan
  staleness, and `fuzzy_find_from_scan` equivalence.
- **Spec-level behavior is unchanged** for the consumer capabilities:
  `notes-history`'s synth exclusion, `link-review`'s callout exclusion,
  and `notes-journal`'s alias resolution keep their requirements — only
  the mechanism moves (CLI stays fresh; the TUI is eventually
  consistent, the same envelope the shared-graph-snapshot capability
  already accepts).

## Out of scope (later changes)

- **CLI `ft find`** — stays a filesystem walk (user decision).
- Full file content in the snapshot; `Task`/`TaskData` model dedup;
  promoting `drift`/`ghosts`; adopting `Graph::refresh_note` as an
  incremental rebuild path.
