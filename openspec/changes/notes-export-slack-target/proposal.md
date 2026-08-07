## Why

`ft notes export` (landed 2026-08-05) has one target: `commonmark`.
Notes are increasingly shared in Slack, whose mrkdwn dialect is
deliberately incompatible with CommonMark: `#` headings, `**bold**`,
`[text](url)` links, images, tables, and task checkboxes all render as
literal text. The capability spec already names `slack` as a planned
target behind the `ExportTarget` seam — this change implements it, so
`ft notes export --format slack` produces text that actually renders
correctly when pasted into Slack.

## What Changes

- **New `--format slack` target** for `ft notes export` (default stays
  `commonmark`). The mapping, agreed with the user:
  - **Passes through / reuses**: frontmatter clamp + `--lines` range
    semantics (target-independent), `[!ft-source]` header drop,
    wikilink → plain text, blockquotes, lists, code spans and fenced
    code blocks, inline code, emoji.
  - **Converts CommonMark → mrkdwn**:
    - Markdown links `[text](url)` → `<url|text>`; non-URL targets
      (internal note links) → display text only. Markdown images
      `![alt](src)` → bare URL when `src` is http(s) (Slack unfurls a
      preview), else alt text. Obsidian embeds `![[img.png]]` → plain
      filename/display text (vault-local files are unreachable from
      Slack).
    - Headings `# H` → `*H*` (bold; level lost — Slack has no
      headings).
    - Emphasis: `**bold**` → `*bold*`, `*italic*` → `_italic_`,
      `~~strike~~` → `~strike~`, `***both***` → `*_both_*`. Backtick
      code spans untouched; CommonMark flanking rules keep `snake_case`
      and `2 * 3` literal.
    - Callouts: `> [!note] Title` → `> Title` (the `[!type]` marker is
      vault chrome; the title survives as the quote's first line).
    - Task lines: `- [ ] ⏫ …` → `- ⏫ …` (checkbox dropped, emoji
      metadata kept, indent preserved, `[x]` state dropped).
    - Code fences: language tags stripped from opening fences
      (```` ```rust ```` → ```` ``` ````); `~~~` tilde fences converted
      to ` ``` ` (Slack doesn't know tildes).
  - **Left raw (deliberately, per decision)**: `&`, `<`, `>` are not
    escaped — the output targets the Slack composer (pasting), where
    HTML entities would show literally. Tables and `---` rules stay
    literal (documented limitation).
- **Driver extension**: `LineContext` gains an `opened_fence` flag so
  the Slack target can distinguish opening fence delimiters (language
  tag stripping) from closing ones and content lines. Targets stay
  stateless `&'static` impls; `commonmark` is unaffected.
- **CLI**: `ExportFormat::Slack` variant (value name `slack`); unknown
  values still rejected (`plaintext` remains unimplemented).
- **Spec/docs/tests**: capability spec delta, `docs/guide/notes.md`
  mapping section, unit tests (per-line transform table) + integration
  tests (byte-exact Slack fixture).

## Capabilities

### New Capabilities
<!-- none -->

### Modified Capabilities
- `notes-export`: the `--format` contract gains `slack`; the wikilink
  conversion, pass-through, and Slack-specific transformation rules
  become target-parametric (commonmark vs slack).

## Impact

- `ft-core/src/export.rs` — new `SlackExport` impl + shared inline
  scanner; `LineContext` field added.
- `ft-core/src/markdown.rs` — `LineSkipState` gains an
  `opened_fence` accessor (fence open/close detection already exists
  internally).
- `ft/src/cmd/export.rs` — `ExportFormat::Slack` variant + `--format`
  help text.
- `ft/tests/notes_export.rs` — Slack fixture tests; existing
  `--format plaintext` rejection test stays valid.
- `openspec/specs/notes-export/spec.md` (delta at archive),
  `docs/guide/notes.md`.
- No new dependencies. No CLI contract churn for existing users
  (`--format commonmark` unchanged).
