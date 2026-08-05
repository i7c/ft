## Context

Every note in an ft vault carries vault-specific structure that is
meaningless outside the vault: the leading YAML frontmatter block,
`[!ft-source]` provenance callouts (the protected-section grammar from
`ft_core::synth::callout`), and `[[wikilinks]]` that only resolve
inside Obsidian. `ft notes quote` (landed 2026-08-04) wraps a raw line
range *into* a pinned callout; `ft notes export` is the inverse — a
read-only plumbing command that renders a note (or a range of it) as
clean, portable CommonMark.

The user asked for this first target to be built on a minimal,
extensible architecture: export to plain text and Slack are already
planned, so the stripping rules must live behind a seam, not inline in
the command. The established precedent is the
`task::format::TaskFormat` / `EmojiFormat` trait seam: consumers see
the trait, one impl is v1, and detection/config wires it later.

## Goals / Non-Goals

**Goals:**
- `ft notes export <FILE> [--lines A-B] [--format commonmark]`:
  read-only, no git, no writes, no prompts, plain stdout.
- Strip exactly: the frontmatter block, `[!ft-source]` callout header
  lines (body lines survive as blockquotes), and `[[wikilinks]]`
  (converted to plain text / CommonMark images).
- `--lines` addresses the **original file**; the start clamps to the
  first line after frontmatter so vault structure can never leak into
  the export.
- Target-extensible architecture in ft-core: one `ExportTarget` trait,
  per-line `transform_line` semantics, `CommonMark` as the v1 impl.
- Stable CLI contract: `--format` flag exists today (default
  `commonmark`) so plaintext/slack add enum variants + impls, not
  contract churn.

**Non-Goals:**
- **No full Obsidian-to-CommonMark sanitization.** Only the listed
  elements are touched; everything else (task emoji, `> [!note]`
  callouts, `==highlight==`, markdown links) passes through verbatim.
  "Valid CommonMark" means the *transformations* never produce
  non-CommonMark output — not that every Obsidian-ism is normalized.
- **No git interaction.** Nothing is pinned, verified, or checked for
  cleanliness; the working tree is the source of truth.
- **No attachment export.** Embeds become CommonMark image references;
  no files are copied or inlined.
- **No block-level target semantics in v1.** The per-line
  `transform_line` is the only seam. A future target needing block
  context (e.g. Slack fenced blocks) can extend the trait with a
  defaulted block method without breaking the CLI contract.
- **No JSON/structured output, no color, no prompts.** The cleaned
  markdown is the contract, like quote's raw callout.

## Decisions

### D1. Core module: `ft_core::export` with a per-line `ExportTarget` trait

**Decision:** New `ft-core/src/export.rs`:

```rust
/// One export target: renders a single source line (no trailing
/// newline) into its exported form, or drops it (`None`).
pub trait ExportTarget {
    /// Stable name for `--format` and diagnostics.
    fn name(&self) -> &'static str;
    /// Render one line for this target. `None` drops the line.
    fn transform_line(&self, line: &str) -> Option<String>;
}

/// v1 target: clean CommonMark.
pub struct CommonMarkExport;
```

`CommonMarkExport::transform_line`:
1. If the line matches the canonical `[!ft-source]` header grammar
   (`synth::callout::header_regex`) → `None` (drop).
2. Otherwise → the line with wikilinks/embeds converted (D4), via a
   module-private per-line scanner. Blockquote `> body` lines of a
   dropped callout survive untouched (they are already CommonMark
   blockquotes); wikilinks inside them are converted like any other
   line.

**Rationale:** Mirrors the `TaskFormat`/`EmojiFormat` seam — consumers
take `&dyn ExportTarget`; adding plaintext/slack later is a new impl +
a `--format` enum variant. Per-line granularity keeps the frontmatter
clamp and range slice target-independent (both are line-level), and
covers the whole v1 surface.

**Alternatives considered:** *Whole-document transform trait.* Rejected
for v1: the range clamp, callout drop, and wikilink conversion are all
line-scoped; a document-level trait would push the shared range logic
into every impl. *Enum-with-match instead of trait.* Rejected: the
`TaskFormat` precedent is a trait, and new formats are additive —
trait impls don't require touching a central match. (The CLI-side
`--format` parse is an enum, mirroring `output::Format`.)

### D2. Driver: `export_content` — frontmatter clamp + range, shared by all targets

**Decision:**

```rust
pub struct ExportOutcome { pub text: String }

pub enum ExportError {
    /// `B` beyond the file's raw line count.
    RangePastEnd { file_lines: u32, requested_end: u32 },
}

pub fn export_content(
    content: &str,
    range: Option<(u32, u32)>,       // original-file, 1-indexed inclusive
    target: &dyn ExportTarget,
) -> Result<ExportOutcome, ExportError>
```

Pipeline:
1. `first = frontmatter_end_line(content).map(|f| f + 1).unwrap_or(1)`
   — the first body line (1 when no frontmatter).
2. Range `None` → `(1, count_lines(content))`; `Some((a, b))` →
   validate `b <= count_lines(content)` (else
   `ExportError::RangePastEnd`), then clamp `a = max(a, first)`.
3. `a > b` (range fully inside frontmatter, or empty after clamp) →
   empty `ExportOutcome` (exit 0, nothing printed — the picked lines
   were vault structure, deliberately dropped).
4. `slice_lines(content, a, b)` (the shared `synth::slice` primitive)
   → transform each line via `target.transform_line`, skipping `None`,
   join with `\n`.

**Rationale:** `--lines` numbers are original-file lines (the user's
explicit contract), so the range validation and the error report use
the *raw* line count — same message shape as quote. Only the start
clamps, and only for frontmatter: callouts no longer affect line
numbering (their headers drop in place, bodies stay), so no other
element shifts the numbering.

**Alternatives considered:** *Clamp the end to the file's last line.*
Rejected by the user (Q-A): a `B > N` is a mistake, and silently
truncating could mislead; erroring matches quote. *Strip the blank
line after frontmatter.* Rejected by the user (Q5): the output must
respect the raw range line-for-line.

### D3. Frontmatter end-line helper

**Decision:** Add `pub fn frontmatter_end_line(content: &str) -> Option<u32>`
to `ft_core::frontmatter.rs` — the 1-indexed line number of the
closing fence of a well-formed leading frontmatter block, `None` when
there is none. Line-based: line 1 (trimmed) is `---` opens the block;
the first later line that trims to `---` or `...` closes it
(recognizing the same shapes `markdown::LineSkipState` accepts).

**Rationale:** The existing `frontmatter_block` returns the block
*text*, not its line span; export needs the closing-fence line to
compute the clamp. A line-based helper keeps the recognition rules
consistent with the vault's other parsers.

### D4. Wikilink / embed conversion rules (CommonMark)

**Decision:** Per-line scanner (module-private) that walks the line,
skips inline code spans (single/double/triple backtick runs — the same
convention as `graph::parser::scan_line`), and rewrites:

| Source | Output |
|---|---|
| `[[T]]` | `T` |
| `[[T\|D]]` | `D` |
| `[[T#A]]` | `T` |
| `[[T#A\|D]]` | `D` |
| `[[#A]]` | `#A` |
| `[[#A\|D]]` | `D` |
| `![[T]]` | `![T](href)` |
| `![[T\|D]]` | `![D](href)` |
| `![[T#A]]` | `![T](href)` |
| `![[T#A\|D]]` | `![D](href)` |

`href` is `T` wrapped in `<...>` when it contains whitespace (the
CommonMark angle-bracket form, which `graph::parser` already reads).
Empty bodies (`[[]]`), `[[\|D]]` (no target, no anchor), and
unterminated `[[` are left verbatim — matching the graph parser's
"not a real link" rules. Markdown links `[x](y)` and images
`![alt](src)` are never touched (only `[[` / `![[` trigger the
scanner). The scanner runs on every non-dropped line — including
blockquote lines — so `> [!note]` callouts and kept ft-source body
lines get their wikilinks converted too (user Q4).

**Rationale:** Matches the user's examples exactly (`[[Some other
file]]` → `some other file`; `[[#Heading]]` → `#Heading`; embeds →
CommonMark images). The display text wins when present — it is the
text the author chose to show.

**Alternatives considered:** *Reuse `graph::parser::extract_links`.*
Rejected: it skips blockquote lines (the opposite of what export
needs) and only records occurrences — it does not rewrite. A
dedicated scanner is ~40 lines and owns its own tests. *Drop embeds
entirely.* Rejected by the user (Q3): convert to CommonMark images.

### D5. CLI surface and shared `parse_range`

**Decision:** New `ft/src/cmd/export.rs`:

```
ft notes export <FILE> [--lines A-B] [-l A-B] [--format commonmark]
```

- `<FILE>`: vault-relative or absolute (relativized), no `.md`
  auto-append — same contract as quote.
- `--lines A-B`: **optional**, 1-indexed inclusive, original-file
  lines, short alias `-l`. Absent → whole file (start still clamped).
- `--format <TARGET>`: `ValueEnum`, default `commonmark`, the only
  value today. `ExportFormat::CommonMark` maps to
  `&CommonMarkExport`.
- Output: the transformed text joined with `\n`, printed with a single
  trailing newline; empty result prints **no bytes** (exit 0).
- Exit: 0 on success; 1 with a message on stderr for: no vault, file
  missing/unreadable, invalid `--lines`, or `B` past the end
  ("line range L…-… outside file `…` (file has N lines)", raw count).

Quote's private `parse_range` (`A-B` parse; `A >= 1`, `A <= B`,
positive integers) moves to `cmd::common::parse_line_range` so export
shares it byte-for-byte; `quote.rs` is refactored to call it (its
tests stay green unchanged).

**Rationale:** Mirrors the quote landing: a thin CLI module registered
under `NotesCommand::Export`, domain logic in ft-core. The `--format`
flag exists now (user Q-C) so the contract is stable; `ValueEnum`
gives free validation and completion entries.

## Risks / Trade-offs

- **[Risk] `parse_range` extraction touches quote.** → Mitigation:
  behavior-preserving move; `ft/tests/notes_quote.rs` stays green
  unchanged, and the parse tests move with the function.
- **[Risk] Wikilink scanner diverges from the graph parser** (it does
  scan blockquote lines and treats `[[#A]]` as convertible, where the
  graph parser ignores both). → Mitigation: deliberate, documented
  differences; the scanner has its own unit-test table covering every
  conversion form, code spans, and the "not a real link" edges.
- **[Risk] A range starting mid-callout yields an orphan blockquote**
  (header outside the range, body lines inside it) or a body with its
  first line cut (header inside, body continues past `B`). →
  Accepted: the per-line transform is the contract, and the output is
  always valid CommonMark either way.
- **[Trade-off] Per-line trait limits block-level target semantics
  (e.g. Slack code fences).** → Accepted for v1; the trait can gain a
  defaulted block-context method later without breaking the CLI
  contract or the v1 impl.
- **[Trade-off] CRLF files are not normalized** (lines keep `\r`,
  matching `slice_lines` and quote). → Accepted: vault files are
  conventionally LF; noted in docs rather than special-cased.
- **[Trade-off] `[[Foo]](bar.md)` — a markdown link whose display is
  itself a wikilink — gets its inner wikilink converted.** → Accepted:
  the graph parser treats it the same way; vanishingly rare in real
  vaults.

## Migration Plan

N/A — additive CLI surface. No config, no data migration, no existing
behavior changes (the only edit to existing code is the
behavior-preserving `parse_range` extraction).

## Open Questions

None — all decisions resolved with the user:
- Range basis: original-file lines; start clamped to first line after
  frontmatter; `B` past the end errors with the raw count (Q-A);
  all-frontmatter ranges → empty output, exit 0 (Q-B).
- Callouts: headers dropped, `> body` lines kept as blockquotes
  (explicit user correction); malformed headers kept as blockquotes.
- Wikilinks: brackets stripped; `[[#A]]` → `#A`; embeds →
  CommonMark images; conversion applies inside kept blockquotes.
- Frontmatter: no blank-line stripping — the raw range is respected.
- `--format commonmark` flag ships now with the default (Q-C).
