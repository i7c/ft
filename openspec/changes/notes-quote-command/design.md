## Context

The protected-section pinning mechanics live in
`ft_core::synth::scaffold::plan_synth_scaffold`, shared by `ft notes
synth scaffold` and `ft notes synth grow`. Given `SynthSource`
(`source_path`, `line_start`, `line_end`, `body`), it (a) refuses
sources that are modified/deleted/conflicted/untracked in the working
tree (`Error::SynthDirtySources`), (b) pins to HEAD's short SHA, (c)
computes the 6-hex blake3 content hash of the body, and (d) builds a
`ProtectedSection`, which `synth::callout::serialize` renders as the
`> [!ft-source] "<vault-rel-path>" La-b @sha7 #hash6` blockquote.

Two gaps motivate a plumbing command:

1. **No CLI surface exposes just this mechanics.** `ft.nvim` (the
   motivating consumer) wants to pin arbitrary editor selections as
   protected sections, but the only producers go through the full
   gather/journal flow into a *target note* with editor handoff. There
   is no read-only way to get "the callout this file+range would pin."
2. **The line-range slice is duplicated and working-tree-less.** The
   "split on `\n`, validate a 1-indexed inclusive range, rejoin with
   `\n`" operation exists 4× in core (`verify.rs`, `repair.rs` ×2,
   `reslice.rs`), all blob-side, all with subtly different edge
   behavior (trailing-newline files: `"a\nb\n".split('\n')` yields
   `["a","b",""]`, so naive `end > lines.len()` validation has an
   off-by-one at the last line — `verify`'s current check lets an
   out-of-range pin through in a corner case). No code reads the
   *working tree* at a range, which `quote` needs.

The callout grammar, verification semantics, and scaffold/grow
behavior are frozen; this change adds a surface and refactors internals
without altering any of them.

## Goals / Non-Goals

**Goals:**
- `ft notes quote <file> --lines A-B`: read-only plumbing that emits
  the canonical protected-section callout for an arbitrary 1-indexed
  inclusive line range, on stdout, with nothing written anywhere.
- Quote runs the *same* pinning code as scaffold/grow — shared
  primitives in ft-core, enforced by construction.
- Fix the latent line-range edge cases by centralizing the slice
  operation in core, used by both the new working-tree path and the
  existing blob paths (`verify` at minimum; `repair`/`reslice` adopt
  it opportunistically).
- Stable, minimal contract for ft.nvim: raw markdown callout, no
  color, no prompts, deterministic exit codes.

**Non-Goals:**
- **No JSON / structured output.** The raw callout is the contract;
  consumers needing structured data already have `ft notes synth
  verify --json`.
- **No file writes, target note, dedup, watermark, or editor
  handoff.** All of `plan_synth_scaffold`'s target-note machinery is
  out of scope; quote is single-section emit-only.
- **No paragraph model.** Quote slices *arbitrary* line spans
  (`markdown::extract_paragraphs` and the graph are not involved).
- **No whole-tree clean guard.** The prerequisite is per-source-file
  (the pinned file is committed and unmodified), matching the core
  planner — not the TUI's whole-tree entry fast-fail.
- **No callout grammar / hash-constant changes.**
- **No `repair`/`reslice` refactor requirement** — they adopt
  `slice_lines` only if it falls out naturally; their behavior is
  preserved either way.

## Decisions

### D1. New core primitive `synth::slice`
**Decision:** New `ft_core/src/synth/slice.rs`:

```rust
/// Number of lines in `content`. A trailing newline is not an extra
/// line ("a\nb\n" has 2 lines, "" has 0).
pub fn count_lines(content: &str) -> u32

/// Slice `content`'s lines `start..=end` (1-indexed inclusive),
/// rejoined with `\n`, no trailing newline. Returns `None` when the
/// range is empty or out of bounds (validate with [`count_lines`]).
pub fn slice_lines(content: &str, line_start: u32, line_end: u32) -> Option<String>
```

Line counting: `split('\n')`, then drop the trailing empty element
when `content` ends with `'\n'` (so `"a\nb\n"` counts 2 lines, not 3).
Validation: `line_start >= 1`, `line_start <=
line_end <= line_count`. Body = `lines[(start-1)..end].join("\n")`.

All four existing slice sites adopt it (user decision: reduce
code duplication):

- `verify.rs::verify_one` — blob slice; its `SourceMissing` "line
  range outside file" detail is preserved (message uses `count_lines`).
- `reslice.rs` — `file_lines` becomes `count_lines(&blob)` (this also
  fixes its phantom line: `resolve_range` was bounds-checking against
  a trailing-newline-inflated `lines.len()`); the new body slice and
  the `healed_drift` comparison both become `slice_lines` calls
  (`None` for the old range ⇒ already broken ⇒ `healed_drift = true`,
  matching today's out-of-bounds branch).
- `repair.rs` — `body_matches_pin`'s manual slice + bounds guard
  becomes `slice_lines(blob, start, end) == Some(body)` (`None` ⇒ no
  match, same as today's guard). `find_body`'s needle search is *not*
  a range slice and stays as-is.
- `quote` uses it working-tree-side.

**Rationale:** One definition of the semantics, with the trailing-
newline fix, used by the emitter (quote), the checker (verify), and
both re-slicers means every consumer agrees on what a "line range"
means — self-consistency by construction. This is the exact "bundle
it in core" the user asked for: the slice is domain logic, and four
blob-side copies already existed before quote.

**Alternatives considered:** *Inline a 4-line slice in `cmd/quote.rs`.*
Rejected: it would be the 5th copy, with the same trailing-newline
trap, and nothing would force quote's output and verify's checks to
agree. *Put the helper on `Vault` as `read_lines`.* Rejected: a
`Vault` method that both reads the file *and* slices couples I/O to the
slice and would not serve the blob-side consumers; the pure
string→range function is the honest primitive, and reading stays at the
call site.

### D2. Shared pin-building primitives in `synth::scaffold`
**Decision:** Extract the section-building core of
`plan_synth_scaffold` into two shared functions in `scaffold.rs`:

```rust
/// Run `git status` once and return the sorted, deduped subset of
/// `paths` (vault-relative, like `SynthSource.source_path`) that are
/// modified/deleted/conflicted/untracked.
pub fn find_dirty_sources(repo: &git::RepoMap, paths: &[PathBuf]) -> Result<Vec<PathBuf>>

/// Pure: build the pinned section (hash + HEAD short SHA + struct).
pub fn build_pinned_section(short_sha: &str, entry: &SynthSource) -> ProtectedSection
```

Plus a small `git::head_short_sha(repo) -> Result<String>` (the
`head_sha[..7.min(len)]` truncation, currently inline in
`plan_synth_scaffold`).

`plan_synth_scaffold`'s batch path becomes:
`find_dirty_sources(&repo, &all_sources)` → error with the full
offender list (unchanged error shape) → `head_short_sha` once → loop
via `build_pinned_section`. `quote` composes the same three:
`find_dirty_sources` on its single path (same
`Error::SynthDirtySources`, one element) → `head_short_sha` →
`build_pinned_section` → `serialize` → stdout.

**Rationale:** "Quote is exactly what scaffold does" becomes a
property of shared code, not a claim. Keeping the batch check in
`plan_synth_scaffold` (rather than making quote and scaffold both call
a `pin_section(vault, entry)` per-entry function) avoids N `git status`
+ N `git rev-parse` subprocess spawns for multi-section notes and
preserves the better all-offenders error message.

**Alternatives considered:** *One `pin_section(vault, &SynthSource)`
composing everything, used by both.* Rejected: scaffold's loop would
re-run repo discovery/status/rev-parse per section. *Duplicate the
logic in `cmd/quote.rs`.* Rejected: exactly the drift this change is
meant to eliminate.

### D3. CLI surface: `ft notes quote`
**Decision:** New variant `NotesCommand::Quote(QuoteArgs)` in
`ft/src/cmd/notes.rs`, dispatched to a new `ft/src/cmd/quote.rs`
module (registered in `cmd/mod.rs`):

```
ft notes quote <FILE> --lines A-B
```

- `<FILE>`: vault-relative path, taken as given (no `.md`
  auto-append — it is a *source* path like scaffold's `--from`, not a
  target). Absolute paths are accepted and passed through
  `vault.relativize` so the callout header is always vault-relative.
- `--lines A-B` (short alias `-l A-B`): required, 1-indexed
  inclusive, parsed with the same rules as `synth reslice --lines`
  (positive integers, `A <= B`).
- Output: the canonical callout (`callout::serialize`) plus one
  trailing newline, on stdout. No color, no prompts, no `--json`.
- Exit: 0 on success; 1 with a message on stderr for: vault has no git
  repo, file missing/unreadable, source dirty, or range out of bounds
  (message includes the actual line count).

Run order: discover vault → discover repo → read file → clean check →
slice → build → serialize → print. Reading before the clean check gives
the cheapest, clearest failure for the common "typo'd path" case; an
untracked new file is caught by the clean check (it exists, so it
reaches the check, and untracked ∈ dirty).

**Rationale:** Placement under `notes` keeps the synth family
together in the CLI surface the user asked for (`ft notes quote`),
consistent with `gather`/`recent`/`pulse` being read-only note-surface
commands. A new module keeps `notes.rs` from growing further.

**Alternatives considered:** *Subcommand of `synth`
(`ft notes synth quote`).* Rejected by the user: `ft notes quote` is
the requested surface, and it reads like the other read-only
note-level commands. *Reuse `normalize_md_target`.* Rejected: that
helper exists for synth *target* notes; quote is a source-path command
and auto-appending `.md` to a path a caller (ft.nvim) passes verbatim
would be surprising.

### D4. Prerequisite semantics: per-source clean, matching the core
**Decision:** The only cleanliness requirement is the *pinned file*:
committed at HEAD and unmodified in the working tree (i.e. not in
`git status`'s modified/deleted/conflicted/untracked sets). Other dirty
files in the tree do not block quote — the same contract
`plan_synth_scaffold` enforces.

**Rationale:** This is the actual prerequisite for a verifiable pin
(the HEAD blob must reproduce the body). The whole-tree guard exists
only as the TUI paragraph-synth *entry* fast-fail (UX, not semantics);
a plumbing command should not be stricter than the engine it exposes.

**Alternatives considered:** *Whole-tree `is_clean()` guard.* Rejected
by the user in review: per-source is what scaffold/grow enforce, and a
stricter check would reject quote calls in trees where synth works
fine.

## Risks / Trade-offs

- **[Risk] Refactor ripple in scaffold/verify tests.** →
  Mitigation: `find_dirty_sources`/`build_pinned_section` are
  behavior-preserving extractions; existing tests (batch dirty error
  shape, hash-pins-to-HEAD, verify round-trip) must stay green
  unchanged. `slice_lines` gets its own unit tests covering the
  trailing-newline and out-of-bounds cases.
- **[Risk] `verify`'s blob slice behavior changes for pathological
  ranges** (a pin whose range includes the phantom empty line of a
  trailing-newline file would now report out-of-range instead of
  drifting; `reslice`'s `resolve_range` similarly rejects such ranges).
  → Mitigation: no engine-produced pin can have such a range (bodies
  never include the phantom line), so this only affects hand-crafted
  malformed pins; it is the deliberate correctness fix. Verify's
  round-trip tests and reslice's tests cover the normal envelope.
- **[Risk] ft.nvim builds on an unstable stdout contract.** →
  Mitigation: the `notes-quote` spec pins the exact output (one
  callout, trailing newline, no other bytes), and the ft.nvim change is
  tracked as a `[ft.nvim]`-tagged task in this change's task list with
  the paired-commit note.
- **[Trade-off] No structured output means scripts that want
  path/range/hash must parse the header.** → Accepted per decision
  (user: "I don't see how json adds value for now"); the header
  grammar is already the machine-readable contract elsewhere
  (`callout::parse` round-trips it).
- **[Trade-off] `repair`'s `find_body` needle search keeps its own
  `split('\n')` line-vector handling.** → Accepted: it is a body-search
  operation (exact/whitespace-insensitive match over the whole blob),
  not a range slice; forcing `slice_lines` onto it would be wrong.
  Everything that *slices a range* — verify, reslice, repair's
  `body_matches_pin`, quote — now shares one helper.

## Migration Plan

N/A — additive CLI surface + behavior-preserving refactor. No config,
no data migration. The only behavioral delta anywhere is the `verify`
edge-case fix noted above, which only affects malformed hand-crafted
pins.

## Open Questions

None — both implementation-time questions are resolved: `--lines` gets
the short alias `-l` (user decision), and `reslice` + `repair` adopt
`slice_lines` in this change (user decision, see D1).
