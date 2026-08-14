# notes-export-slack-list-indent — tasks

## 1. Core: list-depth tracker + driver plumbing

- [ ] 1.1 Add a `ListDepthTracker` to `ft-core/src/markdown.rs`
      (pub(crate), next to `LineSkipState`): holds a `Vec<usize>` stack
      of source-indent widths; `advance(&mut self, line) ->
      Option<usize>` returns the nesting depth for a list-item line and
      `None` otherwise, per design D2 (empty stack → push, depth 0;
      `w > top` → push/nest; `w == top` → same level; `w < top` → pop
      to matching level or between-level push). Leading width counts
      columns with tabs advancing to the next multiple of 4 (same rule
      as `starts_with_indent`). A non-list, non-blank line with zero
      leading whitespace resets the stack (design D3). List-item
      detection: after leading whitespace, `-`/`*`/`+` or `\d+.`
      followed by space/tab. Unit tests in `markdown.rs`: the full
      depth walk of `- foo`/`  - bar`/`    - lol`/`- baz` (0/1/2/0),
      4-space source walk (0/1/2), sibling-after-deeper pop, mixed
      `*`/`+`/`1.` markers, reset on heading/paragraph, no reset on
      blank lines, tabs counted at 4-column stops, first-line-indented
      fragment starts at depth 0.
- [ ] 1.2 Extend `LineContext` in `ft-core/src/export.rs` with
      `list_depth: Option<usize>` (default `None`, `Default` derive
      unchanged) and plumb it in `export_content`: after
      `ls.skip_line(line)`, advance the tracker only when
      `!ls.last_was_code()` (design D3) and store the result.
      `CommonMarkExport` ignores the field. Unit tests: code lines
      (fence delimiters, in-fence content, indented code) yield
      `None`; a list item inside a fence yields `None`; `LineContext`
      default is `None`; the whole-document driver walk produces the
      expected per-line depths.

## 2. Core: Slack target re-indentation

- [ ] 2.1 In `SlackExport::transform_line`, after the existing
      structural rewrites, apply a re-indent step: when
      `ctx.list_depth` is `Some(d)` **and** the transformed line still
      starts (after leading whitespace) with a list marker, replace
      the leading whitespace with `" ".repeat(4 * d)`; otherwise the
      line passes through unchanged (design D1/D4, defensive marker
      re-check). Unit tests in `export.rs` via the `LineContext`
      helper: `  - bar` at depth 1 → `    - bar`; `    - lol` at
      depth 2 → `        - lol`; depth 0 unchanged; checkbox-dropped
      task line `- [ ] foo` at depth 1 → `    - foo`; non-list line at
      depth `None` unchanged; `> - foo` blockquote line at depth `None`
      unchanged; marker-preserving transforms (`- **bold**` →
      `    - *bold*`) keep their content converted.
- [ ] 2.2 Whole-document slack-driver tests in `export.rs` for the
      spec scenarios: the user's 2-space example (0/4/8/0), deep
      nesting (0/4/8/12), all marker kinds, idempotence on 4-space
      sources, list-looking lines inside fenced and indented code
      blocks untouched, list interrupted by a heading resets nesting,
      and `commonmark` output byte-identical for the same fixtures.

## 3. Core: nested task checkbox fix

- [ ] 3.1 Widen `drop_task_checkbox` in `ft-core/src/export.rs` from
      the `i < 3` leading-space cap to scanning all leading spaces and
      tabs (design D4), keeping every existing guard (marker char,
      whitespace after marker, `[ ]`/`[x]`, whitespace-or-EOL after
      the bracket). Unit tests: `    - [ ] foo` → `    - foo`,
      `        - [x] done` → `        - done`, `\t- [ ] tab` →
      `\t- tab`, and the existing no-false-positive cases
      (`- [foo]`, `[x]foo`) still pass.
- [ ] 3.2 Update the slack transform-level and integration-test
      expectations that involve nested task lines: `  - [x] done` now
      exports as `    - done` (checkbox dropped + depth-1 re-indent).

## 4. Integration tests (`ft/tests/notes_export.rs`)

- [ ] 4.1 Update the `slack_export_converts_every_construct` fixture's
      expected output: the `- [ ] ⏫ …` / `  - [x] done` pair now
      exports as `- ⏫ 📅 2026-08-05 Finish` / `    - done`.
- [ ] 4.2 Add integration tests mirroring the spec scenarios:
      two-space list re-indented (the user's exact example), all
      marker kinds, deep nesting, idempotence, code blocks protected,
      heading interruption reset, nested task checkbox at 4+ spaces,
      and a commonmark-unchanged guard for a 2-space nested list.

## 5. Docs

- [ ] 5.1 Update `docs/guide/notes.md` §"Exporting for Slack": add a
      bullet describing 4-space-per-level list indentation
      normalization (all marker kinds, code protected, interruption
      reset) and the nested task-checkbox drop; note the limitations
      (continuation lines and `> - foo` lists keep source
      indentation).

## 6. Build invariants

- [ ] 6.1 Verify the five clean:
      `cargo build --release`, `cargo test --workspace`,
      `cargo clippy --workspace --tests -- -D warnings`,
      `cargo fmt --check`,
      `cargo run --release -q -- commands docs --check`.
