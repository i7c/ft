# Migrating graph DSL edge kinds

The graph DSL's `edge.kind` value set changed in the contents-graph
refactor. This is a **hard break** — old `link` / `embed` values no
longer parse (the error lists the allowed set). Translate your
presets, scripts, and TUI snippets using the table below.

## Why

Links are now modeled at three container levels — note, heading, and
paragraph — sharing one `LinkEdge` payload and identical resolution
semantics. Embed-ness moved from a separate edge kind to a property of
the link occurrence (`edge.embed`). See `docs/graph-semantics.md` for
the full model.

## Translation

| Old                       | New                                                  | Notes |
|--------------------------|------------------------------------------------------|-------|
| `edge.kind = link`        | `edge.kind = note-link`                              | Note-level link (all occurrences). For heading/paragraph-sited links use `heading-link` / `paragraph-link`. |
| `edge.kind = embed`       | `edge.embed = true`                                 | Embed-ness is now a boolean predicate on any link kind. Optionally AND with a `edge.kind` filter. |
| `edge.kind in {link, embed}` | `edge.kind = note-link`                           | `note-link` covers all occurrences; add `and edge.embed = true` to restrict to embeds. |

The full new `edge.kind` value set: `note-link`, `heading-link`,
`paragraph-link`, `directory-contains`, `has-task`, `subtask`,
`links-into`, `owns-paragraph`, `owns-heading`. New node kind:
`Heading` (queryable via `kind = Heading`, with a `title` attribute
holding the heading text).

## Examples

Old:

```dsl
node where kind = Note;
expand where edge.kind in {link, embed};
```

New (all note-level links):

```dsl
node where kind = Note;
expand where edge.kind = note-link;
```

New (embeds only):

```dsl
node where kind = Note;
expand where edge.kind = note-link and edge.embed = true;
```

New (paragraph-sited links — what the journal / related-updater use):

```dsl
node where kind = Paragraph;
expand where edge.kind = paragraph-link;
```
