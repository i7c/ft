# notes-export-slack-target — tasks

## 1. Core: driver extension + shared scanner helpers

- [x] 1.1 Extend `LineContext` in `ft-core/src/export.rs` with
      `opened_fence: bool` (default `false`) and plumb it in
      `export_content`; add `opened_fence` tracking to
      `LineSkipState` in `ft-core/src/markdown.rs` (set true exactly
      on the line that opens a fence — the branch in `classify` where
      `self.fence = Some(c)`; false on every other line including the
      closing delimiter) with a `pub(crate) fn opened_fence()` style
      accessor. `CommonMarkExport` ignores the field. Unit tests:
      opening delimiter with info string, bare opening delimiter,
      closing delimiter, content line inside a fence, indented-code
      lines, and `LineContext::default()` stays `false`.

## 2. Core: SlackExport target

- [x] 2.1 Implement `SlackExport` in `ft-core/src/export.rs`:
      `name() == "slack"`; `transform_line` drops canonical
      `[!ft-source]` header lines (same `header_regex()` as
      CommonMark); code lines pass through verbatim except fence
      normalization (2.4); otherwise run the Slack inline scanner
      (2.2) after structural rewrites (2.3). Unit tests for the
      drop/pass-through boundary: canonical header dropped, malformed
      header kept then marker-stripped, body `> ` lines kept,
      code-context lines verbatim.
- [x] 2.2 Slack inline scanner (single left-to-right pass, reusing the
      existing backtick-run helpers `run_len`/`closing_run_end` and
      the wikilink body parser `split_wiki`/`wikilink_end`): code
      spans copied verbatim; wikilinks → plain text (shared table:
      `[[T]]`→`T`, `[[T|D]]`→`D`, `[[T#A]]`→`T`, `[[#A]]`→`#A`); embeds
      `![[…]]` → display text or trimmed target (anchor dropped);
      markdown links `[text](url)` → `<url|text>` for `http(s)`/`mailto`
      URLs (title dropped), display text only otherwise; markdown
      images `![alt](src)` → bare URL for `http(s)` src, alt text
      otherwise; emphasis delimiters per CommonMark flanking rules
      with first-match pairing (`***x***`→`*_x_*`, `**x**`→`*x*`,
      `*x*`→`_x_`, `_x_`→`_x_`, `~~x~~`→`~x~`). Unit tests: the full
      mapping table, code spans untouched, flanking edges (`snake_case`,
      `2 * 3`), `***` runs, multiple constructs per line, unicode
      surroundings, non-link edges (`[[]]`, unterminated `[[`).
- [x] 2.3 Structural line rewrites (applied to the raw line before the
      inline scanner, per design D2): ATX heading prefix `#{1,6} ` →
      `*` with a closing `*` appended (level lost); blockquote callout
      marker strip — after the `>` prefixes, a leading `[!word]` token
      (letters/digits/`-`/`_`) followed by whitespace or EOL is
      removed, on any blockquote depth; task checkbox drop — `- ` /
      `* ` / `+ ` + `[ ]`/`[x]` → the bullet char alone (indent and
      rest of line kept). Unit tests: `# H`→`*H*`, `## H`→`*H*`,
      heading with trailing `#`, `> [!note] Title`→`> Title`,
      `> > [!note] x`→`> > x`, `> [!note]`→`> `, `- [ ] …`→`- …`,
      `  - [x] done`→`  - done`, no-false-positive (`- [foo]` not a
      task, `[!note]` mid-line not stripped).
- [x] 2.4 Fence normalization using `ctx.opened_fence` and
      `ctx.in_code`: opening backtick fence with a language tag
      (backticks followed by non-backtick, non-whitespace content) →
      bare ` ``` `; opening or closing `~~~`-fence delimiter line →
      ` ``` `; every other line passes through. Unit tests: ` ```rust `
      → ` ``` `, `~~~`→` ``` ` both delimiters, content lines inside
      fences untouched, a ` ```js `-style content line inside a fence
      is NOT treated as a delimiter (the block stays intact), tilde
      fence with info string, indented code untouched.

## 3. CLI

- [x] 3.1 Add `ExportFormat::Slack` to `ft/src/cmd/export.rs` with
      `#[value(name = "slack")]` and a doc comment; map it to
      `&SlackExport` in `ExportFormat::target()`; update the
      `--format` help text and the `ExportArgs` docs so the target
      list reads `commonmark` + `slack` (plain text still planned,
      not accepted).

## 4. Integration tests

- [x] 4.1 Extend `ft/tests/notes_export.rs` with a `slack` fixture
      note combining every mapped construct (frontmatter, ft-source
      callout, wikilinks, embeds, markdown links incl. internal and
      remote-image forms, headings, bold/italic/strike, code spans,
      fenced block with language tag, tilde-fenced block, task lines
      `[ ]`/`[x]`, `> [!note]` callout, `snake_case`/`2 * 3` literal
      guards, `& < >` raw) asserting a byte-exact expected slack
      output; `--format slack` accepted while `--format plaintext`
      still rejected; `-l` range behavior identical across targets;
      a commonmark export of the same fixture unchanged (regression
      guard).

## 5. Docs

- [x] 5.1 Document the `--format slack` target in `docs/guide/notes.md`
      (the mapping table: what converts, what stays literal, the
      no-escaping rationale, code-fence normalization); update the
      sentence that says slack is "planned"; check
      `docs/architecture.md` and the README for stale "planned
      targets" mentions.

## 6. Verification

- [x] 6.1 Run the five build invariants: `cargo build --release`,
      `cargo test --workspace`, `cargo clippy --workspace --tests -- -D
      warnings`, `cargo fmt --check`,
      `cargo run --release -q -- commands docs --check`.
- [x] 6.2 Commit the implementation as its own commit (the spec commit
      already lands separately).
