## Why

The protected-section pinning mechanics live deep inside
`plan_synth_scaffold`: given a source file + line range that is
committed at HEAD, core builds a `[!ft-source]` callout (7-hex HEAD
short SHA + 6-hex blake3 content hash) that `ft notes synth verify`
can later validate. There is no CLI surface that exposes *just* this
mechanics — scripts and external tools (notably `ft.nvim`, which wants
to pin arbitrary editor selections) can't produce a protected section
without either going through the full gather/journal scaffold flow or
hand-writing callouts by guesswork. We need a plumbing command:
`ft notes quote <file> --lines A-B` → raw callout on stdout, read-only.

## What Changes

- **New subcommand `ft notes quote <file> --lines A-B`**: pure,
  read-only plumbing. It validates the source file is tracked and
  unmodified at HEAD (the same per-source clean check scaffold/grow
  enforce), validates the file exists and the 1-indexed inclusive
  range is in bounds, slices the body verbatim from the working tree,
  builds the protected section exactly like scaffold/grow (HEAD short
  SHA, blake3 content hash), and prints the raw markdown callout to
  stdout. No file writes, no target note, no editor handoff, no JSON.
- **Core refactor so quote shares scaffold's mechanics by
  construction**, not by copy:
  - New `ft_core::synth::slice` helper `slice_lines(content, start,
    end)` — the 1-indexed-inclusive line-range slice + bounds
    validation, currently duplicated 4× blob-side across
    `verify`/`repair`/`reslice` with subtly different edge behavior
    (trailing-newline off-by-one). `verify` is refactored to use it;
    `quote` uses it working-tree-side.
  - Extract the per-source pin construction from
    `plan_synth_scaffold` into a shared single-source primitive
    (single-source clean check + HEAD SHA + hash + `ProtectedSection`).
    Behavior-preserving for scaffold/grow (batch clean-check error
    shape unchanged).
- **Docs**: new command documented; architecture/guide notes updated.

## Capabilities

### New Capabilities
- `notes-quote`: the `ft notes quote` command contract — input
  contract (vault-relative file + `A-B` 1-indexed inclusive range),
  prerequisites (source committed and unmodified at HEAD; file exists;
  range in bounds), pin semantics (identical to scaffold: HEAD short
  SHA + blake3 6-hex prefix, serialized via the canonical callout
  grammar), and the read-only/stdout output contract.

### Modified Capabilities
- None. The callout grammar, verify semantics, and scaffold/grow
  requirements are unchanged — the core refactor is behavior-
  preserving, so no existing spec needs a delta.

## Impact

- `ft-core/src/synth/`: new `slice.rs`; `scaffold.rs` gains the
  single-source pin primitive + shared clean-check helper (batch
  behavior unchanged); `verify.rs` refactored to `slice_lines`.
- `ft/src/cmd/`: new `quote.rs` module; `NotesCommand::Quote` variant
  + dispatch in `notes.rs`; module registered in `cmd/mod.rs`.
- `docs/`: new-command coverage in the guide + architecture docs.
- `ft.nvim`: the motivating consumer — an editor-side task (tagged
  `[ft.nvim]`) in the sibling repo will call `ft notes quote` to pin
  visual selections; this change only guarantees the stable CLI
  contract it needs.
