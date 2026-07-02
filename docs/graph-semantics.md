# Graph Semantics

This document is the canonical reference for how `ft`'s in-memory note
graph models the **contents** of notes — the notes themselves, their
headings, their paragraphs, and the links between them. It defines the
intended semantics: what each node and edge *means*, independent of the
exact Rust enum shape or storage strategy.

Companion docs:

- `docs/architecture.md` — the "Graph query DSL" / "Profiles and the
  unified DSL" sections describe how the graph is *queried*.
- `docs/graph-query-dsl.md` — the DSL grammar, attribute matrix, kind
  values, and worked examples.
- `openspec/specs/graph-model/spec.md` — the formal requirements and
  scenarios (supersedes the narrower `paragraph-graph` spec).

For "how it works today" (pre-refactor), see the summary captured in the
openspec change that accompanies this doc.

## Scope and status

This document specifies the **target semantics** after the contents-graph
refactor. Sections marked *Status: new* or *Status: changed* describe
behavior that differs from the current implementation and is the subject
of the refactor. Everything else is existing behavior restated for
completeness.

## Guiding rules

Two rules govern the model:

1. **Hierarchical structures are modeled hierarchically.** A heading
   begins a new paragraph, and further paragraphs can follow under that
   heading. Lower-level headings nest under higher-level headings. The
   graph represents this topology with edges: notes point to their
   headings and (heading-less) paragraphs; headings point to their
   sub-headings and their paragraphs. Containment is **exclusive** —
   every paragraph and heading has exactly one owner (its nearest
   enclosing heading, or the note if there is none).

2. **Link duplication is acceptable across levels when it carries
   distinct analytical signal and is consistent.** Links are recorded
   at three levels — note, heading, and paragraph — because it is
   genuinely interesting both *which paragraphs* contain a link (for
   the journal, related-scoring, link review) and *which notes* do (for
   backlinks, rename). A third level — heading links — is added: a link
   occurring on a heading line. Because every heading line is also part
   of a paragraph, a heading-link overlaps with a paragraph-link and a
   note-link; that overlap is intentional and acceptable. **All three
   link levels share identical link semantics** (same forms, same
   resolution rules, same anchor handling, same ghost keying).

## Two edge families: structure vs. reference

The model rests on a clean asymmetry:

- **Structure (containment) edges** model document topology. Each node
  has **one** owner (nearest container). Exclusive, tree-shaped.
  Kinds: `Contains`, `OwnsHeading`, `OwnsParagraph`, `HasTask`,
  `Subtask`, `LinksInto` (derived).

- **Reference (link) edges** model citations between notes. Naturally
  many-to-many and **duplicated across the three container levels**.
  Same semantics at every level. Kinds: `NoteLink`, `HeadingLink`,
  `ParagraphLink` (each carrying the shared `LinkEdge` payload).

Containment is exclusive because structure is a tree; links are
duplicated because references are a graph. This split is the heart of
the model and resolves the tensions in the pre-refactor design (where
links were modeled twice with *different* semantics — note-level
wiki+md, paragraph-level wiki-only and data-less).

## Node kinds

| Kind | Payload | What it represents |
|---|---|---|
| `Note(NoteData)` | `path: PathBuf`, `title: String` (filename stem) | A markdown file. Carries no body text directly — its contents live in its child `Heading` and `Paragraph` nodes and its outgoing link edges. |
| `Heading(HeadingData)` | `source_file: PathBuf`, `line: u32`, `level: u8`, `text: String` | An ATX heading line. `text` is the heading text with leading `#`s and trailing `#`s/whitespace stripped (matches `markdown::Heading.text`). **Status: new.** |
| `Paragraph(ParagraphData)` | `source_file: PathBuf`, `line_start: u32`, `line_end: u32` (1-indexed inclusive), `text: String` | A blank-line-delimited block of prose. Unchanged from today. |
| `Task(TaskData)` | description, status, priority, dates, tags, `source_file`, `source_line` | A task line. Unchanged. |
| `Ghost(GhostData)` | `raw: String` | An unresolved link target. Shared across all linkers to the same unresolved string. Unchanged. |
| `Directory(DirData)` | `path`, `name` | A vault directory. Unchanged. |

### Note

A note is identified by its vault-relative path. Its `title` is the
filename stem (no extension) — used for wikilink title resolution. A
note's body is the union of its transitively-owned headings and
paragraphs; it has no `text` field of its own.

### Heading (*Status: new*)

A heading node is created for every ATX heading (`#`…`######`) found
in a note's content, using the existing `markdown::extract_headings`
extractor (which already skips frontmatter, fenced code, and indented
code). Identity is `(source_file, line)` — stable across rebuilds via
`NodeKey::Heading(path, line)`.

Per guiding rule 1 + Fork A2, the heading line **also belongs to the
paragraph that begins at that line** (the heading is the start of a new
paragraph, and that paragraph's `text` still includes the heading line,
verbatim, exactly as `extract_paragraphs` produces today). So the
heading line is *dual-owned* in the content sense: structurally by the
heading node, textually by the paragraph node. This is intentional and
keeps every existing consumer of `ParagraphData.text` (journal feed,
synth callouts, related scoring) unchanged.

### Paragraph

Unchanged. `extract_paragraphs` splits content at blank lines, ATX
heading lines, and `--`/`---` horizontal rules; frontmatter and
fenced/indented code blocks are skipped via `LineSkipState`. A heading
line starts a new paragraph and is absorbed into it (its `text` begins
with the heading line).

### Task

Unchanged and **intentionally outside the heading/paragraph containment
hierarchy.** Tasks carry their own `HasTask` (note → top-level task) and
`Subtask` (task → task, indentation-derived) tree. A task that
physically appears under a heading is *not* given a heading→task edge in
this revision. Folding tasks into the heading tree is a possible future
extension but is out of scope here; it would interact with the
emoji-format task model and is deferred.

### Ghost, Directory

Unchanged.

## Structure edges (containment, exclusive)

Containment edges have **no payload** — they are pure topology. Each
owned node has exactly one incoming containment edge from its nearest
container.

### `Contains` — directory → note | directory

Unchanged. A directory contains its immediate child notes and
subdirectories.

### `OwnsHeading` — note → heading | heading → heading (*Status: new*)

Models the heading section tree. A heading `H` at level `L` is owned by
the nearest enclosing heading of level `< L`, or by the note if there is
no such heading. Build-time algorithm (a heading stack):

1. Maintain a stack of open headings, each with its level.
2. For a heading `H` at level `L`: pop every heading with level `≥ L`
   (its section has closed). `H`'s parent is the new top of stack, or
   the note if the stack is empty. Add `OwnsHeading(parent → H)`. Push
   `H`.
3. Process headings in document order.

This yields Obsidian's fold semantics: an `## Section` owns its `###`
subsections and everything between them up to the next `##` (or `#`).

### `OwnsParagraph` — note → paragraph | heading → paragraph (*Status: changed*)

A paragraph is owned by its **nearest enclosing heading**, or by the
note if it is not under any heading (e.g. intro text before the first
heading). This replaces the pre-refactor behavior where every paragraph
was owned directly by its note.

Build-time rule: when a paragraph `P` starts at line `l`, its parent is
the heading currently on top of the heading stack at line `l`, or the
note if the stack is empty. Add `OwnsParagraph(parent → P)`.

**Ordering invariant:** because a heading line is also a paragraph's
first line (Fork A2), the heading must be pushed onto the stack *before*
the paragraph starting at the same line is processed, so that
intro-paragraph is owned by its own heading. Concretely: within one
note, process headings and paragraphs in ascending `line` order, and
when a heading and a paragraph share a start line, process the heading
first.

**Consequence (known, accepted):** a lone heading line with no body
(e.g. `## A\n### B`) produces a one-line paragraph whose `text` is the
heading line itself, owned by that heading. Consumers may filter such
single-heading-line paragraphs if they are too noisy, but the graph
models them faithfully per Fork A2.

### `HasTask` — note → task

Unchanged. Note → its top-level tasks only.

### `Subtask` — task → task

Unchanged. Indentation-derived, always intra-file.

### `LinksInto` — note → directory (*derived, unchanged*)

One edge per unique (source-note, target-directory) pair derived from
resolved link/embed edges. Unchanged.

### Reading "all paragraphs of a note"

With exclusive ownership, `graph.outgoing(note).filter(OwnsParagraph)`
returns **only the note's heading-less paragraphs** — paragraphs under
headings are reachable only through the heading tree. The canonical
traversal is recursive:

```
all_paragraphs(note) = OwnsParagraph children of note
                    ∪ ⋃ OwnsParagraph children of every transitively-
                       OwnsHeading-descendant heading
```

A `Graph::note_paragraphs(note_id)` helper centralizes this and replaces
the flat `outgoing(note).filter(OwnsParagraph)` walk used today by
`journal`, `related`, and `link_review`. A `Graph::note_headings(note_id)`
helper returns the direct `OwnsHeading` children (with a recursive
variant for the full subtree).

## Reference edges (links, duplicated)

Links are recorded at three container levels. **All three carry the
full `LinkEdge` payload and share identical link semantics.**

| Kind | Source → Target | Meaning |
|---|---|---|
| `NoteLink(LinkEdge)` | note → note/ghost | One edge per link occurrence anywhere in the note. The multiset of all link occurrences in the note. |
| `HeadingLink(LinkEdge)` | heading → note/heading/ghost | One edge per link occurrence on the heading line itself. |
| `ParagraphLink(LinkEdge)` | paragraph → note/heading/ghost | One edge per link occurrence in the paragraph body. |

**Status:** `NoteLink` generalizes today's `Link`/`Embed` (see "Embed
handling" below). `HeadingLink` is new. `ParagraphLink` is changed from
a unit edge to a full `LinkEdge`-carrying edge, and now includes
**markdown-form links and embeds** (today it is wiki-only and data-less).

### The shared `LinkEdge` payload

Every link edge — regardless of level — carries:

| Field | Meaning |
|---|---|
| `form: LinkForm` | `WikiLink` (`[[…]]`) or `MdLink` (`[…](href)`) |
| `is_embed: bool` | `true` for `!`-prefixed transclusions (`![[…]]`, `![…](…)`) |
| `byte_range: Range<usize>` | Byte range in the source file's content at parse time |
| `line: usize` | 1-indexed source line |
| `raw_text: String` | Verbatim source token, e.g. `"[[Foo|alias]]"` |
| `target_text: String` | Pre-pipe, pre-anchor target (raw target for wiki; URL-decoded href for md) |
| `anchor: Option<String>` | Post-`#` heading anchor, if any |
| `display: Option<String>` | Post-`|` alias (wiki) or bracketed text (md); `None` when absent |

**Status: changed.** Today `LinkEdge` has `form` but not `is_embed`
(embed-ness is encoded in the `Embed` edge *variant*). Under the unified
model, embed-ness moves onto `LinkEdge` as data so all three levels
treat embeds uniformly. See "Implementation latitude" for the encoding
choice.

### Shared link semantics

All three link kinds resolve identically. Resolution is performed once
per occurrence (in the parse phase) and the resulting target is stored
on the edge; the rules are unchanged from today's `graph::resolve`:

- **Wikilink with `/`** → vault-relative path lookup, with `.md`
  fallback. Hit ⇒ resolved note; miss ⇒ ghost keyed by the verbatim
  target text.
- **Wikilink without `/`** → title (filename stem) lookup. Zero ⇒
  ghost; one ⇒ resolved; **>1 ⇒ shortest path, alphabetical tiebreak**
  (Obsidian's "shortest path when possible").
- **Markdown link** → URL-decoded href resolved relative to the
  linker's directory, `.md` fallback, `..`/`.` normalized. Miss ⇒
  ghost keyed by the normalized vault-relative path string.
- **External URLs** (`http://`, `https://`, `mailto:`, `obsidian://`,
  `ftp://`, …) are filtered at the parser and never become edges.

### Anchor handling (*Status: new — anchors now resolve*)

`LinkEdge.anchor` (the `#heading` part) is parsed today but never
consulted. Under the new model anchors become real:

- A link **without** an anchor targets the note (or ghost) as today.
- A link **with** an anchor `[[Foo#Bar]]`:
  - If `Foo` resolves to a note **and** `Bar` matches a heading in that
    note → the edge targets the **heading node** `H(Bar)`.
  - If `Foo` resolves to a note but `Bar` matches no heading → the edge
    targets the **note** `Foo`, with `anchor = Some("Bar")` retained as
    metadata.
  - If `Foo` does not resolve → the edge targets the **ghost** keyed by
    `Foo` (the note target); the anchor dangles as metadata.

Heading matching for anchors is case-insensitive, ignoring leading/
trailing whitespace and trailing `#`s (consistent with
`markdown::Heading.text`). Slug/normalization details (whitespace
collapse, punctuation) are specified in the openspec design.

**Interpretive call (flagged for confirmation):** the heading-target
choice means "incoming links to this heading" becomes a first-class
query — the main payoff of heading nodes. It also means consumers that
treat any `[[Foo…]]` as a mention of note `Foo` (journal, related) must
union incoming-to-`Foo` with incoming-to-`Foo`'s headings. The
`Graph::mentions_of(note_id)` helper centralizes this.

### Overlap and the derivation contract (guiding rule 2)

The three link levels overlap by construction:

- A link on a heading line is **all of**: a `HeadingLink` (from the
  heading), a `ParagraphLink` (from the paragraph that begins at that
  heading line, per Fork A2), and a `NoteLink` (from the note).
- A link in a paragraph body is a `ParagraphLink` and a `NoteLink`.
- Every `NoteLink` occurrence corresponds to exactly one
  `HeadingLink` or `ParagraphLink` occurrence (whichever container the
  line falls in).

This is the intended duplicity of rule 2. The **semantic contract** is:

> `NoteLink` is exactly the multiset of all link occurrences in the
> note. `HeadingLink` is the multiset of occurrences on heading lines.
> `ParagraphLink` is the multiset of occurrences on paragraph lines
> (including heading lines, since those start paragraphs). The three
> are mutually consistent: `NoteLink == HeadingLink ⊎ ParagraphLink`
> as multisets of occurrences.

**Derivation is implementation-optional.** The implementation may store
all three levels physically, or store only the most specific
(heading/paragraph) and derive `NoteLink` on demand, or store only
`NoteLink` per-occurrence and derive the others. What matters is that
the externally observable semantics match the contract above. The
implementation choice is recorded in the openspec design; see
"Implementation latitude."

### Ghost keying

Unchanged in spirit: a ghost is keyed by the unresolved **note** target
(the pre-anchor, pre-pipe target for wikilinks; the normalized
vault-relative path for markdown links). Anchors do not participate in
ghost identity — `[[Missing#Bar]]` and `[[Missing]]` share one ghost
keyed by `Missing`. Ghost GC remains: a ghost is removed when its last
incoming link edge (at any of the three levels) is removed.

## Embed handling (*Status: changed*)

Today embeds are a separate `EdgeKind::Embed` variant parallel to
`Link`. Under the unified model, embed-ness is a property of the link
**occurrence** (the `is_embed` field on `LinkEdge`), not a separate
topology. A `![[Foo]]` in a paragraph is a `ParagraphLink` whose
`LinkEdge.is_embed == true`. Embeds participate fully at all three
levels and resolve identically to non-embed links.

This is a breaking change to the DSL `edge.kind` value set (see
"Query DSL exposure"). The implementation encoding is a design decision
(see "Implementation latitude").

## Identity and stable keys

Unchanged in shape, extended for headings:

- **Internal:** `NoteId` (newtype over petgraph `NodeIndex`, stable
  across removals via `StableGraph`). Not stable across separate
  `Graph::build` calls.
- **External / cross-build:** `NodeKey`, now with a `Heading` variant:

| `NodeKey` variant | Identity |
|---|---|
| `Note(PathBuf)` | vault-relative path |
| `Directory(PathBuf)` | vault-relative path; root = empty |
| `Ghost(String)` | verbatim unresolved target |
| `Task(PathBuf, usize)` | (source_file, 1-indexed source_line) |
| `Paragraph(PathBuf, u32)` | (source_file, 1-indexed line_start) |
| `Heading(PathBuf, u32)` | (source_file, 1-indexed heading line) — **new** |

Side tables on `Graph`: `path_index`, `title_index`, `ghost_index`,
`task_index`, `paragraph_index` (all unchanged), plus a new
`heading_index: HashMap<(PathBuf, u32), NoteId>` keyed by
`(source_file, line)`. Lookups: `note_by_path`, `note_by_title`,
`ghost_by_raw`, `task_by_loc`, `paragraph_by_loc` (unchanged), plus
`heading_by_loc(path, line)` (new).

## Build and refresh invariants

The parse pass lives in `Vault::scan`: one parallel read per file
extracting tasks, links, headings, and paragraphs together into
`Scan::files` (headings already come from `extract_headings` in the
same `LineSkipState`-aware scan). `Graph::build` consumes those
artifacts and does no file I/O of its own — `scan → build` reads each
vault file exactly once. The serial resolution phase, in order:

1. Insert note nodes.
2. Insert directory nodes + `Contains` edges.
3. Insert link edges at the note level (so the path/title indexes are
   populated for heading-anchor resolution in step 5).
4. Insert heading nodes + `OwnsHeading` edges (heading-stack algorithm).
5. Insert paragraph nodes + `OwnsParagraph` edges (nearest-container
   rule), and link edges at the heading and paragraph levels. Anchor
   resolution happens here (heading indexes are now populated).
6. Insert task nodes + `HasTask` + `Subtask` edges.
7. Derive `LinksInto` edges.

`Graph::refresh_note` removes a note's headings, paragraphs, and all
outgoing link edges (all three levels), GCs orphaned ghosts across all
three levels, then re-inserts from the file's current content. The
heading-stack and nearest-container rules re-run for that file.

`byte_range` on every `LinkEdge` indexes the source file's content at
parse time — re-parse (`refresh_note`) before relying on it after any
edit, as today.

## Query DSL exposure

The DSL gains a `Heading` node kind and restructured link edge kinds.

| Attribute | `Note` | `Heading` (new) | `Paragraph` | `Task` | `Directory` | `Ghost` |
|---|---|---|---|---|---|---|
| `kind` | `Note` | `Heading` | `Paragraph` | `Task` | `Directory` | `Ghost` |
| `path` | ✓ | None | None | source file | ✓ | None |
| `title` | filename stem | heading text | None | None | None | None |
| `indegree`/`outdegree` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |

Link edge kind values (replaces today's `link` / `embed`):

| `edge.kind` value | Edge |
|---|---|
| `note-link` | `NoteLink` |
| `heading-link` | `HeadingLink` |
| `paragraph-link` | `ParagraphLink` |

`edge.form` remains `wiki` | `md`. A new `edge.embed` boolean (`true` |
`false`) replaces the `embed` edge kind. Existing presets/queries using
`edge.kind = link` or `edge.kind = embed` require migration (see openspec
design). `path` on `Heading`/`Paragraph` returns `None` as today
(`kind` is the filter); exposing `source_file` via `path` there is a
possible future extension, intentionally deferred to avoid changing
`path includes "…"` semantics for paragraphs.

## Consumer contracts (migration)

Every graph consumer is updated to the new semantics. The contract each
relies on:

| Consumer | Old | New |
|---|---|---|
| `rename` (`plan_rename`) | rewrites via note-level `Link`/`Embed` `byte_range` | rewrites via note-level link edges (`NoteLink`), which carry full per-occurrence data including `byte_range`. **Unchanged in practice** if `NoteLink` is stored per-occurrence (the recommended encoding). If `NoteLink` is derived, rename iterates `HeadingLink` + `ParagraphLink` and dedups by `byte_range`. |
| `journal` (`build_journal`) | `ParagraphLink` (wiki-only, unit) + `ParagraphData.text` | `ParagraphLink` (all forms, full data); paragraph traversal via `note_paragraphs(note)`; mentions of a note via `mentions_of(note)` (union of incoming-to-note + incoming-to-headings). `section_text` unchanged (A2 preserves `ParagraphData.text`). **Behavior change:** markdown links now count as mentions. |
| `related` (`score_related`) | `ParagraphLink` (wiki-only) co-occurrence | Same traversal changes as journal; co-occurrence now includes markdown links. **Behavior change:** more co-occurrences surface. |
| `link_review` | `OwnsParagraph` + `ParagraphLink`, line-overlap | Paragraph walk via `note_paragraphs`; `ParagraphLink` carries data; callout-skip by line range unchanged. New heading nodes do not directly affect the diff→paragraph mapping. |
| `synth::scaffold` | consumes `JournalEntry.section_text` | Unchanged — `section_text` derives from `ParagraphData.text`, which A2 preserves. |
| `graph::query` | 5 node kinds, 8 edge kinds | 6 node kinds (add `Heading`); link edge kinds restructured (see DSL exposure). Snapshots regenerated. |
| TUI graph tab / `output::graph` | renders 5 node kinds | renders 6 node kinds; `child_sort_key` gains a `Heading` rank between `Directory` and `Note` (or per design). Snapshots regenerated. |

New `Graph` helpers (centralize the traversals every consumer needs):

- `note_paragraphs(note_id) -> Vec<NoteId>` — recursive OwnsHeading walk
  ∪ direct OwnsParagraph.
- `note_headings(note_id) -> Vec<NoteId>` — direct OwnsParagraph…
  OwnsHeading children; plus a recursive variant for the full subtree.
- `mentions_of(note_id) -> impl Iterator<Item=(NoteId, &LinkEdge)>` —
  incoming link edges targeting the note **or** any of its headings, at
  all three levels.

## Implementation latitude

These choices are **not** fixed by this semantics document; they are
decided in the openspec design and may evolve:

1. **Edge enum encoding.** Either (a) three link edge kinds
   (`NoteLink`/`HeadingLink`/`ParagraphLink`) each carrying `LinkEdge`
   with `is_embed` as a field, **or** (b) six kinds splitting embed into
   its own variant per level. Recommended: (a), fewer variants and a
   cleaner `edge.embed` DSL predicate.
2. **Link derivation direction.** Store all three levels physically, or
   store only the most specific (heading/paragraph) and derive
   `NoteLink`, or store only `NoteLink` per-occurrence and derive the
   others. The semantics contract (overlap section) must hold either
   way. The choice trades memory for recomputation; `rename` is
   simplest if `NoteLink` is stored per-occurrence.
3. **Heading `text` storage.** Store the stripped heading text on
   `HeadingData` (recommended, enables `title` queries and anchor
   matching), or derive on demand from `ParagraphData.text`.

## Decisions log

| Decision | Choice | Rationale |
|---|---|---|
| Fork A: heading/paragraph boundary | **A2** — heading line belongs to both heading node and paragraph `text` | Pragmatic; preserves every consumer of `ParagraphData.text` (journal, synth, related) unchanged. |
| Fork B: paragraph ownership | **Exclusive nearest-container** — heading-less paragraphs owned by note, others by nearest heading | Captures true document structure (rule 1); consumers traverse via `note_paragraphs`. |
| Heading-link definition | Links occurring on the heading line itself | Per spec. |
| Anchors | **Implemented** — resolvable anchor targets the heading node; else the note | Makes heading nodes first-class query targets; `mentions_of` helper preserves note-mention semantics. |
| Link forms at all levels | Wiki **and** markdown at note/heading/paragraph levels | "All link implementations compatible and support all link types." Breaking behavior change for journal/related (md links now count). |
| Embed encoding | Embed-ness as `LinkEdge` data (`is_embed`), not a separate edge kind | Uniform treatment across levels; recommended encoding (a). Breaking DSL change. |
| Tasks in heading tree | Out of scope | Avoids coupling with emoji task model; deferred. |
| Anchor target when note unresolved | Ghost keyed by note target; anchor dangles | Ghosts represent unresolved notes, not unresolved sections. |
| `path` on Heading/Paragraph | `None` (as today) | Avoids changing `path includes …` semantics for paragraphs; deferred. |

## Out of scope / future

- Folding `Task` nodes into the heading/paragraph containment tree
  (`heading → task`), with the emoji-format interaction that implies.
- Unifying `notes::extract_sections` (heading-delimited sections, used by
  `move-sections` / `plan_related_update`) with the heading-node model,
  so there is one structural representation rather than two.
- Exposing `source_file` via the `path` attribute on `Heading`/`Paragraph`
  for DSL convenience.
- Reference-style markdown links (`[text][ref]` + `[ref]: url`), still
  out of scope as today.
- Setext headings (`===`/`---` underlines), still out of scope as today.
