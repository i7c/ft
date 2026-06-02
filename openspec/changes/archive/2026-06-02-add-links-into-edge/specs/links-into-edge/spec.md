## ADDED Requirements

### Requirement: LinksInto edge exists in the graph

The graph SHALL support an `EdgeKind::LinksInto` variant representing "the source note links to one or more notes contained in the target directory."

#### Scenario: EdgeKind variant exists
- **WHEN** the graph is built
- **THEN** `EdgeKind::LinksInto` is a defined variant alongside `Link`, `Embed`, `Contains`, and `HasTask`

### Requirement: LinksInto edges created during graph build

After all Link and Embed edges are inserted during `Graph::build()`, the system SHALL insert exactly one `LinksInto` edge per unique (source note, target note's parent directory) pair where the target is a resolved Note node.

#### Scenario: Note links to a note in a subdirectory
- **WHEN** note `a/b/foo.md` contains `[[Bar]]` that resolves to `c/d/e/Bar.md`
- **THEN** a `LinksInto` edge exists from the `a/b/foo.md` Note node to the `c/d/e` Directory node

#### Scenario: Note links to a note at vault root
- **WHEN** note `a/foo.md` contains `[[Index]]` that resolves to `Index.md` (at vault root)
- **THEN** a `LinksInto` edge exists from the `a/foo.md` Note node to the vault-root Directory node

#### Scenario: Embeds also create LinksInto edges
- **WHEN** note `a/foo.md` contains `![[diagram.png]]` that resolves to `images/diagram.png`
- **THEN** a `LinksInto` edge exists from the `a/foo.md` Note node to the `images` Directory node

### Requirement: LinksInto edges are deduplicated per folder

If a source note contains multiple links (Link or Embed) resolving to notes in the same target directory, the system SHALL create at most one `LinksInto` edge for that (source, directory) pair.

#### Scenario: Multiple links to same folder produce one edge
- **WHEN** note `a/foo.md` contains `[[X]]` resolving to `d/X.md` and `[[Y]]` resolving to `d/Y.md`
- **THEN** exactly one `LinksInto` edge exists from `a/foo.md` to the `d` Directory node

#### Scenario: Links to different folders produce separate edges
- **WHEN** note `a/foo.md` contains `[[X]]` resolving to `d1/X.md` and `[[Y]]` resolving to `d2/Y.md`
- **THEN** a `LinksInto` edge exists from `a/foo.md` to the `d1` Directory node AND a separate `LinksInto` edge exists from `a/foo.md` to the `d2` Directory node

### Requirement: Unresolved links are excluded

Ghost (unresolved) link targets SHALL NOT produce `LinksInto` edges, because ghosts have no parent directory.

#### Scenario: Ghost link produces no LinksInto edge
- **WHEN** note `a/foo.md` contains `[[Phantom]]` that does not resolve to any note
- **THEN** no `LinksInto` edge is created pointing from `a/foo.md` to any directory based on that link

#### Scenario: Mix of resolved and unresolved links
- **WHEN** note `a/foo.md` contains `[[Real]]` (resolves to `d/Real.md`) and `[[Phantom]]` (unresolved)
- **THEN** a `LinksInto` edge exists from `a/foo.md` to the `d` Directory node (from `Real`) AND no `LinksInto` edge is created based on `Phantom`

### Requirement: Self-folder links are included

When a note links to another note in the same directory, the system SHALL still create a `LinksInto` edge to that shared parent directory.

#### Scenario: Note links to sibling in same folder
- **WHEN** note `a/b/foo.md` contains `[[Baz]]` that resolves to `a/b/Baz.md`
- **THEN** a `LinksInto` edge exists from `a/b/foo.md` to the `a/b` Directory node

### Requirement: LinksInto edges survive refresh_note

When `Graph::refresh_note` is called for a note, the system SHALL recompute `LinksInto` edges for that note based on its current resolved links: old `LinksInto` edges from that note are removed, and new ones are inserted for the current link set.

#### Scenario: Refresh recomputes LinksInto edges
- **WHEN** note `a/foo.md` initially links to `d1/X.md` (producing a `LinksInto` edge to `d1`), then the file is edited to also link to `d2/Y.md` and `refresh_note` is called
- **THEN** `LinksInto` edges from `a/foo.md` point to both `d1` and `d2` Directory nodes

#### Scenario: Refresh removes stale LinksInto edges
- **WHEN** note `a/foo.md` initially links to `d1/X.md`, then the link is removed from the file and `refresh_note` is called
- **THEN** no `LinksInto` edge exists from `a/foo.md` to `d1`

### Requirement: LinksInto is queryable through the graph DSL

The query DSL SHALL recognize `"links-into"` as a valid value for `edge.kind` in expand blocks and neighbor filters.

#### Scenario: DSL parses links-into in expand
- **WHEN** the query `node where kind = Note; expand where edge.kind = "links-into";` is parsed
- **THEN** parsing succeeds with no error

#### Scenario: DSL parses links-into in neighbor filter
- **WHEN** the query `node where kind = Directory without outgoing(kind = "links-into");` is parsed
- **THEN** parsing succeeds with no error

#### Scenario: DSL rejects unknown edge kind
- **WHEN** the query contains `edge.kind = "nonexistent"`
- **THEN** parsing fails with an `UnknownKindValue` error

### Requirement: CLI output formats support links-into

All five output formats (Tree, JSON, NDJSON, Edges, Markdown) SHALL render `LinksInto` edges with the label `"links-into"`.

#### Scenario: Edges format includes links-into
- **WHEN** a graph walk traverses a `LinksInto` edge and output format is `edges`
- **THEN** the output line contains the label `links-into` between the source and destination node indices
