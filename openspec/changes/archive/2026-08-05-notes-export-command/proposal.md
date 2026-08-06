## Why

Notes in an ft vault carry vault-specific structure — YAML frontmatter,
`[!ft-source]` provenance callouts, and `[[wikilinks]]` — that makes
them unusable outside the vault (pasting into other apps, publishing,
sharing, piping to external tools). There is no read-only command that
renders a note (or a line range of it) as clean, portable CommonMark.
`ft notes quote` goes the *opposite* direction: it wraps a raw range
into a pinned ft callout. We need the inverse — strip the vault, keep
the prose.

## What Changes

- **New subcommand `ft notes export <FILE> [--lines A-B] [--format commonmark]`**:
  read-only plumbing. It reads a vault file from the working tree and
  prints the vault-stripped content to stdout. No git requirement (no
  pinning, no clean-at-HEAD check), no writes, no editor, no prompts.
- **Stripping rules (CommonMark target):**
  - The leading frontmatter block is dropped entirely. `--lines`
    numbers refer to the **original file**; the range start is clamped
    to the first line after the frontmatter closing fence, so a range
    touching frontmatter silently excludes it (and a range fully
    inside frontmatter yields empty output, exit 0).
  - `> [!ft-source] "…" L… @… #…` **header lines are dropped**; the
    `> body` lines are kept as-is — they are already valid CommonMark
    blockquotes. Malformed ft-source headers stay as plain blockquotes.
  - **Wikilinks become plain text**: `[[Foo]]` → `Foo`,
    `[[Foo|Bar]]` → `Bar`, `[[Foo#H]]` → `Foo`, `[[#H]]` → `#H`.
    **Embeds become CommonMark images**: `![[img.png]]` →
    `![img.png](img.png)` (alias → alt text). Applied everywhere
    except inside code spans/fences — including inside kept
    blockquotes. Self-links are treated like any other link.
  - Everything else passes through verbatim: markdown links, headings,
    task lines (emoji metadata included), non-ft callouts (`> [!note]`),
    code fences, paragraphs.
- **Extensible target architecture, not a one-off transform**: a
  `ExportTarget` trait in ft-core with per-line `transform_line`
  semantics (`None` = drop the line); `CommonMark` is the v1 impl.
  The frontmatter clamp + range slice are target-independent logic
  around it. The CLI exposes `--format` (ValueEnum, default
  `commonmark`, the only value today) so `plaintext` / `slack` land as
  new impls + enum variants later without contract churn.
- **Small shared refactor**: quote's private `parse_range` (the `A-B`
  parser with `A >= 1`, `A <= B`) moves to a shared spot so export
  reuses it byte-for-byte; quote's behavior is unchanged.

## Capabilities

### New Capabilities
- `notes-export`: the `ft notes export` command contract — input
  (vault-relative file + optional original-file line range with
  frontmatter start-clamp + `--format`), stripping rules per target
  (v1: CommonMark), output contract (stdout, one trailing newline,
  empty output on all-frontmatter ranges), and error/exit semantics.

### Modified Capabilities
- None. `notes-quote`'s requirements are unchanged (the
  `parse_range` extraction is behavior-preserving plumbing, not a
  spec-level change).

## Impact

- `ft-core/src/`: new `export.rs` (`ExportTarget` trait,
  `CommonMarkExport` v1 impl, `export_content` driver with frontmatter
  clamp + range validation/slicing, wikilink/embed conversion
  helpers). Reuses `synth::callout::header_regex`, frontmatter
  parsing, and the code-span skip conventions from
  `graph::parser`. Registered in `ft-core/src/lib.rs`.
- `ft/src/cmd/`: new `export.rs` module (`ExportArgs` + `run_export`);
  `NotesCommand::Export` variant + dispatch in `notes.rs`; module
  registered in `cmd/mod.rs`; `parse_range` extracted from
  `quote.rs` into shared code (quote refactored to call it).
- `docs/`: new-command section in `docs/guide/notes.md`; CLI-surface
  line in `docs/architecture.md`; README command list if it
  enumerates `ft notes` subcommands.
- Tests: unit tests in `export.rs`; integration tests in
  `ft/tests/notes_export.rs` (assert_cmd + fixture vault);
  `parse_range` tests move with the extraction.
- `ft.nvim`: not touched; the stable CLI contract is the seam a
  future editor-side "export selection" action would consume
  (tracked as a `[ft.nvim]` task if it lands later, per the
  coordination convention).
