# notes-export-unwrap-soft-wraps — tasks

## 1. Core: shared block-classification primitives

- [ ] 1.1 Make `is_rule_separator` in `ft-core/src/markdown.rs`
      `pub(crate)` (no behavior change; currently private, used by
      `extract_paragraphs`). Verify the existing paragraph tests still
      pass.
- [ ] 1.2 Factor the callout-marker detection in `ft-core/src/export.rs`
      into a shared predicate `callout_marker_end(line) ->
      Option<usize>` (byte offset just past the `]` of a `[!type]`
      marker that starts the content after the `>` prefixes of a
      blockquote line), and make `strip_callout_marker` use it — no
      behavior change to the Slack transform; `slack_callout_marker`
      unit tests stay green.

## 2. Core: BlockKind classifier + join pass

- [ ] 2.1 Add `enum BlockKind { Blank, Code, Heading, ListItem,
      Blockquote, Paragraph, Break }` and a `classify(line, in_code)`
      helper in `ft-core/src/export.rs` using the `markdown.rs`
      primitives (`leading_ws`, `is_list_item_marker`, `parse_atx`,
      `is_blockquote_line`, `is_rule_separator`), with a
      `was_callout_title` flag for Blockquote lines (design D2/D4).
      Order matters: rule/heading/blockquote checks before the list
      marker check; code and blank short-circuit first. Unit tests:
      every kind, plus the blockquote-title flag.
- [ ] 2.2 Add `fn unwrap_soft_wraps(&self) -> bool { false }` to
      `ExportTarget`; override to `true` in `SlackExport` (design D3).
      Add a unit test asserting `SlackExport::unwrap_soft_wraps()` is
      true and `CommonMarkExport::unwrap_soft_wraps()` is false.
- [ ] 2.3 Extend `export_content` with an `unwrap: Option<bool>`
      parameter (`None` → the target's `unwrap_soft_wraps()` policy)
      and implement the join pass per design D1/D2/D5: a `pending`
      accumulator (text + `BlockKind` + `was_callout_title` +
      hard-break flag) that absorbs or flushes each source line per
      the merge table; dropped lines (transform → `None`), blank
      lines, code lines, headings, thematic rules, empty `>` lines,
      and hard-break-terminated lines flush/reset; continuation
      content appends with a single space after stripping leading
      whitespace (and `>` prefixes for blockquotes) and trimming the
      pending text's trailing whitespace. Update the existing
      `export` / `slack_export` test helpers to pass `None`; all
      existing export tests must pass unchanged.
- [ ] 2.4 Driver unit tests in `ft-core/src/export.rs` for the spec
      scenarios: wrapped paragraph joins (slack default), wrapped
      list item joins (the user's exact
      `- …and` / `  …to continue` / `  - sub` / `    follows` /
      `- return` example), nested items do not join, blank lines
      separate, hard breaks preserved (trailing `\` and `  `), code
      blocks never joined (fence and indented), callout title does
      not absorb its body, quoted paragraph joins, headings do not
      absorb the following paragraph, commonmark verbatim by default,
      `--unwrap` join for commonmark, mid-block range fragment starts
      fresh, and dropped ft-source headers act as boundaries.

## 3. CLI: unwrap flags

- [ ] 3.1 Add `--unwrap` / `--no-unwrap` to `ExportArgs` in
      `ft/src/cmd/export.rs` (mutually exclusive via
      `conflicts_with`, the `--has-due`/`--no-due` precedent) and
      resolve the effective policy in `run_export`: flag wins, else
      `args.format.target().unwrap_soft_wraps()`; pass
      `Some(effective)` to `export_content`. Help text documents the
      per-target defaults. Add a CLI unit test for the mutual
      exclusion error.

## 4. Integration tests (`ft/tests/notes_export.rs`)

- [ ] 4.1 Add integration tests mirroring the spec scenarios: the
      user's wrapped-paragraph + wrapped-list fixture exports joined
      under `--format slack`; `--no-unwrap` restores verbatim;
      commonmark default byte-identical and `--unwrap` joins;
      `--unwrap --no-unwrap` fails with an error and empty stdout;
      callout title/body and `> Keep me` / `> see Baz` outputs stay
      byte-identical to today; a `--lines` range starting on a
      continuation line exports a fresh logical line.

## 5. Docs

- [ ] 5.1 Update `docs/guide/notes.md` §"Exporting for Slack": add a
      "Soft-break resolution" subsection describing the join
      (paragraphs, list-item continuations, quoted paragraphs), the
      protections (blank lines, headings, code, hard breaks, callout
      titles), the per-target defaults, the `--unwrap` /
      `--no-unwrap` flags, and the documented limitations (lazy
      continuation, setext headings, hard-break markers kept
      verbatim). Update the §"Exporting for Slack" list-indentation
      bullet's continuation-line sentence (continuations now join by
      default).
- [ ] 5.2 Update the README `ft notes export` mention to note the
      Slack unwrap default and the flags.

## 6. Build invariants

- [ ] 6.1 Verify the five clean:
      `cargo build --release`, `cargo test --workspace`,
      `cargo clippy --workspace --tests -- -D warnings`,
      `cargo fmt --check`,
      `cargo run --release -q -- commands docs --check`.
