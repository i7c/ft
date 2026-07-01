# graph-model Specification

## Purpose
TBD - created by archiving change graph-contents-semantics. Update Purpose after archive.
## Requirements
### Requirement: Heading node kind

The graph SHALL support a `NodeKind::Heading(HeadingData)` variant where `HeadingData` carries `source_file: PathBuf` (vault-relative), `line: u32` (1-indexed line of the heading), `level: u8` (ATX level 1..=6), and `text: String` (the heading text with leading `#`s, trailing `#`s, and surrounding whitespace stripped, matching `markdown::Heading.text`). Heading nodes are created from `markdown::extract_headings` (which already skips frontmatter, fenced code, and indented code blocks).

#### Scenario: Heading node exists for each ATX heading
- **WHEN** `Graph::build` processes a markdown file containing `# Top\n## Sub\nbody`
- **THEN** the graph contains two `NodeKind::Heading` nodes: one at level 1 with text `Top`, one at level 2 with text `Sub`, each carrying the owning note's vault-relative path and the heading's 1-indexed line

#### Scenario: Heading text is normalized
- **WHEN** a file contains `##  My Heading  ##`
- **THEN** the heading node's `text` field is `My Heading` (leading/trailing whitespace and trailing `#`s stripped)

#### Scenario: Heading inside fenced code block is not a heading
- **WHEN** a file contains a ```` ``` ```` fenced code block with a `# Fake` line inside it
- **THEN** no heading node is created for that line

### Requirement: Heading node identity and index

A heading node SHALL be uniquely identified by `(source_file, line)` and SHALL be resolvable via `Graph::heading_by_loc(path, line) -> Option<NoteId>`. The graph SHALL maintain a `heading_index: HashMap<(PathBuf, u32), NoteId>` keyed by `(vault-relative source path, 1-indexed heading line)`. `Graph::stable_key` SHALL return `NodeKey::Heading(source_file, line)` for heading nodes, and `Graph::id_for_key` SHALL resolve `NodeKey::Heading` back to the live `NoteId`.

#### Scenario: Lookup by path and line
- **WHEN** `graph.heading_by_loc(&path, line)` is called with a known heading's coordinates
- **THEN** it returns `Some(NoteId)` for that heading node

#### Scenario: Missing heading returns None
- **WHEN** `heading_by_loc` is called with a line number that is not a heading
- **THEN** it returns `None`

#### Scenario: Stable key round-trips across rebuild
- **WHEN** a `Graph` is built twice from the same on-disk state and `stable_key` is taken for a heading in the first graph
- **THEN** `id_for_key` on the second graph resolves that key to the equivalent heading node

### Requirement: OwnsHeading edges model the heading section tree

The graph SHALL contain `EdgeKind::OwnsHeading` edges modeling heading nesting. A heading `H` at ATX level `L` SHALL be owned by the nearest enclosing heading of level `< L`, or by the note if there is no such heading. Build SHALL use a heading-stack algorithm: process headings in document order; for each heading at level `L`, pop every heading with level `>= L` from the stack, set the parent to the new top-of-stack (or the note if empty), add `OwnsHeading(parent -> H)`, then push `H`.

#### Scenario: Top-level heading owned by note
- **WHEN** a note contains `# A` as its first heading
- **THEN** an `OwnsHeading` edge exists from the note node to the heading node for `A`

#### Scenario: Subheading owned by nearest enclosing heading
- **WHEN** a note contains `# A\n## B\n### C`
- **THEN** `OwnsHeading` edges exist: note -> A, A -> B, B -> C (each subheading owned by the nearest shallower heading)

#### Scenario: Sibling heading closes prior section
- **WHEN** a note contains `# A\n## B\n## C`
- **THEN** `OwnsHeading` edges exist: note -> A, A -> B, A -> C (the second `##` closes `B`'s section; `C` is owned by `A`, not `B`)

### Requirement: OwnsParagraph nearest-container ownership

The graph SHALL contain `EdgeKind::OwnsParagraph` edges from each note (or heading) to the paragraphs it directly owns. A paragraph `P` starting at line `l` SHALL be owned by the heading on top of the heading stack at line `l`, or by the note if the stack is empty. Each paragraph SHALL have exactly one incoming `OwnsParagraph` edge (exclusive containment).

#### Scenario: Heading-less paragraph owned by note
- **WHEN** a note contains intro text before its first heading
- **THEN** an `OwnsParagraph` edge exists from the note node to that intro paragraph node

#### Scenario: Paragraph under a heading owned by that heading
- **WHEN** a note contains `# A\nbody paragraph`
- **THEN** an `OwnsParagraph` edge exists from the heading node for `A` to the paragraph node containing `body paragraph` (not from the note)

#### Scenario: Paragraph under a subheading owned by the subheading
- **WHEN** a note contains `# A\n## B\nbody`
- **THEN** an `OwnsParagraph` edge exists from heading `B` to the `body` paragraph

#### Scenario: Exclusive ownership
- **WHEN** a paragraph exists under a heading
- **THEN** exactly one `OwnsParagraph` edge targets that paragraph node (from its nearest container, never from both the heading and the note)

### Requirement: Heading-paragraph ordering invariant at build

When a heading and a paragraph share the same start line (because the heading line begins a new paragraph per Fork A2), the build SHALL process the heading before the paragraph so the paragraph is owned by its own heading rather than by the prior container.

#### Scenario: Heading line starts a paragraph owned by that heading
- **WHEN** a note contains `# A\n## B` (where `## B` both is a heading and starts a new paragraph whose `text` is `## B`)
- **THEN** the paragraph beginning at `## B`'s line is owned by heading `B` (the heading was pushed onto the stack before the paragraph's ownership was resolved)

### Requirement: Note contents traversal helpers

The graph SHALL expose `note_paragraphs(note_id) -> Vec<NoteId>` returning all paragraphs transitively owned by the note (direct `OwnsParagraph` children plus `OwnsParagraph` children of every transitively-`OwnsHeading`-descendant heading), and `note_headings(note_id) -> Vec<NoteId>` returning the note's direct `OwnsHeading` children, and `all_headings(note_id) -> Vec<NoteId>` returning the full heading subtree.

#### Scenario: note_paragraphs returns heading-less and heading-owned paragraphs
- **WHEN** a note has one intro paragraph (note-owned) and one paragraph under a heading
- **THEN** `note_paragraphs(note)` returns both paragraph ids

#### Scenario: note_paragraphs recurses through nested headings
- **WHEN** a note has `# A` (owning paragraph P1) and `# A\n## B` (B owning paragraph P2)
- **THEN** `note_paragraphs(note)` returns P1 and P2

#### Scenario: note_headings returns only direct children
- **WHEN** a note has `# A` (direct) and `# A\n## B` (B is a grandchild)
- **THEN** `note_headings(note)` returns only A, and `all_headings(note)` returns A and B

### Requirement: Unified link edge kinds

The graph SHALL support three reference edge kinds — `EdgeKind::NoteLink(LinkEdge)`, `EdgeKind::HeadingLink(LinkEdge)`, and `EdgeKind::ParagraphLink(LinkEdge)` — replacing the prior `EdgeKind::Link`, `EdgeKind::Embed`, and the data-less `EdgeKind::ParagraphLink`. Each SHALL carry the full `LinkEdge` payload. Each link occurrence in a note produces exactly one edge at each applicable level: a `NoteLink` from the note (always), plus a `HeadingLink` from the heading whose line the occurrence falls on (if on a heading line) or a `ParagraphLink` from the paragraph the occurrence falls in (otherwise, and additionally when on a heading line, since the heading line begins a paragraph per Fork A2).

#### Scenario: Link in paragraph body produces NoteLink and ParagraphLink
- **WHEN** a paragraph body contains `[[Foo]]` resolving to note `Foo.md`
- **THEN** a `NoteLink` edge exists from the note to `Foo`, and a `ParagraphLink` edge exists from the paragraph to `Foo`, both carrying the same `LinkEdge` (byte_range, raw_text, form, anchor, display, is_embed)

#### Scenario: Link on heading line produces all three levels
- **WHEN** a heading line `# See [[Foo]]` exists and `Foo.md` resolves
- **THEN** a `NoteLink` from the note, a `HeadingLink` from the heading, and a `ParagraphLink` from the paragraph that begins at that heading line all target `Foo`

#### Scenario: No separate Embed edge kind
- **WHEN** the graph is built
- **THEN** `EdgeKind` has no `Embed` variant; embed-ness is recorded on `LinkEdge.is_embed`

### Requirement: Shared LinkEdge payload across link levels

Every link edge — `NoteLink`, `HeadingLink`, `ParagraphLink` — SHALL carry a `LinkEdge` with fields: `form: LinkForm` (`WikiLink` or `MdLink`), `is_embed: bool`, `byte_range: Range<usize>` (byte range in the source file at parse time), `line: usize` (1-indexed), `raw_text: String` (verbatim token), `target_text: String` (pre-pipe, pre-anchor target), `anchor: Option<String>`, `display: Option<String>`. The three levels SHALL share identical link resolution semantics.

#### Scenario: Markdown link produces a ParagraphLink with full data
- **WHEN** a paragraph body contains `[Foo](foo.md)` resolving to `foo.md`
- **THEN** a `ParagraphLink` edge exists from the paragraph to `Foo` with `form = MdLink`, `is_embed = false`, and a `byte_range` whose slice equals `[Foo](foo.md)`

#### Scenario: Embed recorded as is_embed data
- **WHEN** a paragraph body contains `![[Foo]]`
- **THEN** the `ParagraphLink` edge (and corresponding `NoteLink`) has `is_embed = true`

### Requirement: Shared link resolution semantics at all levels

All three link kinds SHALL resolve identically: wikilink-with-slash to vault-relative path lookup with `.md` fallback; wikilink-without-slash to title (filename stem) lookup with shortest-path-then-alphabetical tiebreak for multiple matches; markdown-link to URL-decoded href resolved relative to the linker's directory with `.`/`..` normalization and `.md` fallback; external URLs filtered at the parser. Markdown-form links SHALL produce edges at all three levels (NoteLink, HeadingLink, ParagraphLink), not only note-level as in the prior note-level `Link`/`Embed` model.

#### Scenario: Markdown link resolves at paragraph level
- **WHEN** a paragraph contains `[Bar](sub/bar.md)` and `sub/bar.md` exists
- **THEN** a `ParagraphLink` edge exists from the paragraph to the note `sub/bar.md`

#### Scenario: Wikilink title collision uses shortest path at all levels
- **WHEN** a paragraph and a heading both contain `[[Index]]` and multiple `Index.md` notes exist at different depths
- **THEN** both the `ParagraphLink` and `HeadingLink` (and the `NoteLink`) resolve to the same shortest-path note

### Requirement: Anchor resolution targets heading nodes

For a link occurrence with `anchor = Some(a)` on a `HeadingLink` or `ParagraphLink` edge: if the link's note target resolves to a note `N` and `N` contains a heading whose normalized text equals `a` (case-insensitive, whitespace-collapsed, trailing `#`s stripped), the edge SHALL target that heading node. If the note resolves but no heading matches, the edge SHALL target the note `N` with `anchor` retained as metadata. If the note does not resolve, the edge SHALL target the ghost keyed by the note target, with `anchor` retained as metadata. `NoteLink` edges SHALL always target the note (or ghost), ignoring the anchor for target purposes while retaining it as metadata.

#### Scenario: Resolvable anchor targets the heading
- **WHEN** a paragraph contains `[[Foo#Bar]]` and note `Foo.md` has a heading `## Bar`
- **THEN** the `ParagraphLink` edge targets the heading node for `Bar`, not the note `Foo`

#### Scenario: Unresolvable anchor targets the note
- **WHEN** a paragraph contains `[[Foo#Nope]]` and `Foo.md` has no heading `Nope`
- **THEN** the `ParagraphLink` edge targets the note `Foo` with `anchor = Some("Nope")`

#### Scenario: Anchor on unresolved note targets the ghost
- **WHEN** a paragraph contains `[[Missing#Bar]]` and no `Missing.md` exists
- **THEN** the `ParagraphLink` edge targets the ghost keyed by `Missing`, with `anchor = Some("Bar")`

#### Scenario: NoteLink ignores anchor for target
- **WHEN** a paragraph contains `[[Foo#Bar]]` and `Foo.md` has heading `Bar`
- **THEN** the `NoteLink` edge targets the note `Foo` (not the heading), while the `ParagraphLink` targets the heading `Bar`

### Requirement: mentions_of helper

The graph SHALL expose `mentions_of(note_id) -> impl Iterator<Item = (NoteId, &LinkEdge)>` yielding every incoming link edge (at all three levels: NoteLink, HeadingLink, ParagraphLink) whose target is the note OR any of the note's transitively-owned headings. This is the canonical "any `[[Foo…]]` mentions note Foo" traversal.

#### Scenario: Direct link to note is included
- **WHEN** a paragraph has a `ParagraphLink` to note `Foo`
- **THEN** `mentions_of(foo)` yields that edge

#### Scenario: Anchored link to a heading is included
- **WHEN** a paragraph has a `ParagraphLink` to heading `Bar` (owned by note `Foo`, via anchor `[[Foo#Bar]]`)
- **THEN** `mentions_of(foo)` yields that edge (the heading-targeted edge counts as a mention of `Foo`)

#### Scenario: Links to other notes' headings are excluded
- **WHEN** a paragraph has a `ParagraphLink` to heading `Bar` owned by note `Other` (not `Foo`)
- **THEN** `mentions_of(foo)` does not yield that edge

### Requirement: Ghost keying and garbage collection across link levels

A ghost SHALL be keyed by the unresolved note target (pre-pipe, pre-anchor target for wikilinks; normalized vault-relative path for markdown links). Anchors SHALL NOT participate in ghost identity. A ghost SHALL be removed when its last incoming link edge at ANY of the three levels (NoteLink, HeadingLink, ParagraphLink) is removed.

#### Scenario: Anchor does not split ghosts
- **WHEN** two paragraphs contain `[[Missing]]` and `[[Missing#Bar]]` and `Missing.md` does not exist
- **THEN** both `ParagraphLink` edges target the same ghost node keyed by `Missing`

#### Scenario: Ghost kept alive by a heading link
- **WHEN** a ghost's only incoming edge is a `HeadingLink` and that edge is removed during `refresh_note`
- **THEN** the ghost is garbage-collected (no remaining incoming edges at any level)

### Requirement: Build phase ordering

`Graph::build` SHALL run its serial resolution phase in this order: (1) insert note nodes; (2) insert directory nodes and `Contains` edges; (3) insert `NoteLink` edges (resolving against the path/title indexes); (4) insert heading nodes and `OwnsHeading` edges (heading-stack); (5) insert paragraph nodes, `OwnsParagraph` edges (nearest-container), and `HeadingLink`/`ParagraphLink` edges (with anchor resolution against the now-populated heading index); (6) insert task nodes, `HasTask`, `Subtask`; (7) derive `LinksInto` edges from the unified link kinds. No additional file I/O SHALL be required beyond the single existing parallel parse phase (which extracts links, headings, and paragraphs together).

#### Scenario: Anchor resolution sees heading index
- **WHEN** a paragraph contains `[[Foo#Bar]]` and `Foo.md` has heading `Bar`
- **THEN** the `ParagraphLink` targets heading `Bar` (headings were inserted in step 4 before paragraph links in step 5)

#### Scenario: Single parse pass
- **WHEN** `Graph::build` runs
- **THEN** each markdown file is read exactly once during the parallel parse phase, yielding links, headings, and paragraphs together

### Requirement: refresh_note re-inserts headings and all link kinds

`Graph::refresh_note` SHALL remove the note's heading nodes (and their `OwnsHeading`/`OwnsParagraph`/`HeadingLink` edges), its paragraph nodes (and `OwnsParagraph`/`ParagraphLink` edges), and its outgoing `NoteLink` edges, then garbage-collect orphaned ghosts across all three link levels, then re-extract and re-insert headings, paragraphs, and all link kinds from the file's current content. Removed heading/paragraph nodes SHALL be purged from `heading_index`/`paragraph_index`.

#### Scenario: Refresh updates heading count
- **WHEN** a note is edited to add a new `## Section` heading and `refresh_note` is called
- **THEN** the graph contains one more heading node for that note than before

#### Scenario: Refresh cleans stale heading index
- **WHEN** `refresh_note` is called after a heading is deleted from a file
- **THEN** `heading_by_loc` returns `None` for the deleted heading's former coordinates

#### Scenario: Refresh garbage-collects heading-link-only ghosts
- **WHEN** a note's only `HeadingLink` to a ghost is removed by an edit and `refresh_note` is called
- **THEN** the ghost is removed (no remaining incoming edges at any level)

### Requirement: Graph query DSL support for new node and edge kinds

The DSL SHALL accept `Heading` as a `kind` value selecting `NodeKind::Heading` nodes, and `owns-heading`, `note-link`, `heading-link`, `paragraph-link` as `edge.kind` values. The `title` attribute SHALL return the heading text for `Heading` nodes. A new `edge.embed` boolean predicate (`true`/`false`) SHALL replace the prior `edge.kind = embed` form. The prior `link` and `embed` edge-kind values SHALL be rejected at parse time with `UnknownKindValue` listing the allowed set.

#### Scenario: kind = Heading selects heading nodes
- **WHEN** the query `node where kind = Heading;` is run
- **THEN** only `NodeKind::Heading` nodes are selected

#### Scenario: title returns heading text
- **WHEN** the query `node where kind = Heading and title = "Introduction";` is run
- **THEN** heading nodes whose `text` is `Introduction` are selected

#### Scenario: edge.kind = paragraph-link expands from paragraphs
- **WHEN** the query `node where kind = Paragraph; expand where edge.kind = paragraph-link;` is run
- **THEN** expansion yields the notes/headings/ghosts that paragraphs link to

#### Scenario: edge.embed predicate replaces edge.kind = embed
- **WHEN** the query `node where kind = Note; expand where edge.embed = true;` is run
- **THEN** expansion follows only embed (`is_embed = true`) link edges at any level

#### Scenario: Old edge.kind values rejected
- **WHEN** the query contains `edge.kind = link` or `edge.kind = embed`
- **THEN** parsing fails with `UnknownKindValue` listing `{note-link, heading-link, paragraph-link, ...}`

