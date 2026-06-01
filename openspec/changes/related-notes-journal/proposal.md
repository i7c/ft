## Why

Notes in an Obsidian vault accumulate context in isolation: a daily note mentions a project, a meeting note references a concept, a journal entry circles back to a decision — but there is no way to surface that scattered history from the note itself. This change adds two capabilities that turn the vault's link graph into a time-aware context engine: a journal feed that aggregates all paragraph-level mentions of a note across the vault (ordered by when they were written), and an updater that uses co-occurrence frequency to suggest what belongs in a note's Related section.

## What Changes

- New `NodeKind::Paragraph` and two new edge kinds (`OwnsParagraph`, `ParagraphLink`) are added to the existing heterogeneous `Graph` — paragraph nodes participate in the same petgraph `StableDiGraph` as notes, directories, and tasks.
- `Graph::build` and `refresh_note` are extended to extract paragraph nodes and edges in the existing parallel parse phase (no extra I/O).
- The graph query DSL gains node-kind and edge-kind filters for the new kinds.
- A `BlameCache` (`.ft/cache/blame.msgpack`, rmp-serde) maps `(rel_path, HEAD_hash)` to per-line git timestamps, populated lazily on first journal query per file.
- New `ft-core` modules: `markdown::extract_paragraphs`, `git::blame_file`, `journal`, `related`.
- New subcommands: `ft notes journal <note>` (CLI, table + JSON) and `ft notes update-related <note>` (CLI entry point for the TUI modal).
- New TUI modal on the graph tab for interactive Related section editing.

## Capabilities

### New Capabilities

- `paragraph-graph`: Paragraph nodes and edges (`OwnsParagraph`, `ParagraphLink`) in the graph, including extraction during build/refresh and a new `paragraph_index`. DSL node-kind and edge-kind filter extensions.
- `blame-cache`: Lazy, file-keyed git blame cache stored as a single msgpack file in `.ft/cache/`, used to assign dates to paragraph sections.
- `notes-journal`: The `ft notes journal <note>` command — alias resolution via Related section, graph traversal, date lookup via BlameCache, reverse-chronological feed output.
- `related-updater`: Co-occurrence scoring (`score_related`), plan/apply write-back, and the TUI graph-tab modal for interactive Related section updates.

### Modified Capabilities

## Impact

- **`ft-core/src/graph/mod.rs`**: new `NodeKind`, `EdgeKind` variants, `ParagraphData`, `paragraph_index` side-table, extended `build` and `refresh_note`.
- **`ft-core/src/graph/query.rs`**: DSL node-kind (`kind:paragraph` etc.) and edge-kind (`owns-paragraph`, `paragraph-link`) filter support.
- **`ft-core/src/markdown.rs`**: new `extract_paragraphs` function.
- **`ft-core/src/git.rs`**: new `blame_file` function.
- **New**: `ft-core/src/journal.rs`, `ft-core/src/related.rs`, `ft-core/src/blame_cache.rs`.
- **`ft/src/cmd/notes.rs`**: two new `NotesCommand` variants and handlers.
- **`ft/src/tui/tabs/graph.rs`**: new modal overlay for Related section editing.
- **New dependency**: `rmp-serde` (msgpack) in `ft-core/Cargo.toml`.
- All four build invariants (`build --release`, `test --workspace`, `clippy -D warnings`, `fmt --check`) must stay green.
