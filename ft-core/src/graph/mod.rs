//! Note-link graph: nodes are notes (and ghosts for unresolved targets),
//! edges are link occurrences (wikilinks, markdown links, embeds).
//!
//! ## Identity
//!
//! - Internal: [`NoteId`] — a newtype wrapping petgraph's `NodeIndex`.
//!   Stable for the lifetime of one [`Graph`] (we use `StableGraph` so
//!   removals don't reshuffle indices). Cheap to compare and copy.
//! - External: vault-relative [`PathBuf`]. Two side-tables on `Graph`
//!   ([`Graph::note_by_path`] and [`Graph::note_by_title`]) bridge between
//!   the two.
//!
//! ## Heterogeneous from day one
//!
//! v1 has only [`NodeKind::Note`] / [`NodeKind::Ghost`] and
//! [`EdgeKind::NoteLink`], but the enum shape is here so later plans can
//! add `Folder`, `Task`, `Tag`, `FrontmatterValue`, `HasTag` etc.
//! additively without rewriting the graph type. Today the model carries
//! six node kinds (Note, Heading, Paragraph, Task, Ghost, Directory) and
//! three reference edge kinds (NoteLink, HeadingLink, ParagraphLink)
//! sharing one [`LinkEdge`] payload — see `docs/graph-semantics.md`.
//!
//! ## Ghost nodes
//!
//! When a wikilink or markdown link doesn't resolve to a real note, the
//! graph materializes a [`NodeKind::Ghost`] node keyed by the unresolved
//! string and points the edge at it. Multiple linkers to the same
//! unresolved target share one ghost (via `ghost_index`). This unifies
//! traversal — `incoming(ghost)` works exactly like `incoming(note)`,
//! which is what enables "rename a not-yet-created note" in session 3.

pub mod delete;
pub mod parser;
pub mod preset;
pub mod query;
pub mod rename;
pub mod resolve;

#[cfg(test)]
mod tests;

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use petgraph::stable_graph::{NodeIndex, StableDiGraph};
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use rayon::prelude::*;

use crate::error::Result;
use crate::task::Task;
use crate::vault::{Scan, Vault};

/// Stable identity of a node within a single [`Graph`].
///
/// Newtype wrapping petgraph's `NodeIndex`. Stable across removals because
/// the underlying graph is a `StableGraph`. Not stable across separate
/// [`Graph::build`] calls — callers that need cross-build identity should
/// hold the vault-relative `PathBuf` instead and look it up via
/// [`Graph::note_by_path`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NoteId(pub(crate) NodeIndex);

impl NoteId {
    /// Stable numeric handle for this id within its [`Graph`]. Cross-
    /// references in serialized output (e.g. ndjson `parent_id`) use
    /// this to point from one row to another.
    pub fn index(self) -> usize {
        self.0.index()
    }
}

/// Build-independent identity for a graph node. Unlike [`NoteId`],
/// `NodeKey` survives a [`Graph::build`]: the same on-disk node maps
/// to the same key. Used by UI state (expanded paths, selection) so
/// the tree shape is preserved across rebuilds.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NodeKey {
    /// A real note backed by a markdown file. Vault-relative path.
    Note(PathBuf),
    /// A vault directory. Vault-relative path; root = empty path.
    Directory(PathBuf),
    /// An unresolved link target. Verbatim raw target string.
    Ghost(String),
    /// A task in a note. (source_file, 1-indexed source_line).
    Task(PathBuf, usize),
    /// A paragraph in a note. (source_file, 1-indexed line_start).
    Paragraph(PathBuf, u32),
    /// A heading in a note. (source_file, 1-indexed heading line).
    Heading(PathBuf, u32),
}

/// Per-node payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeKind {
    /// A real note backed by a markdown file.
    Note(NoteData),
    /// An unresolved link target with no backing file. Rewritten by
    /// `plan_rename` just like a real note (session 3).
    Ghost(GhostData),
    /// A vault directory. Contains notes and subdirectories via
    /// [`EdgeKind::Contains`] edges.
    Directory(DirData),
    /// A task extracted from a note. Connected to its source note via
    /// [`EdgeKind::HasTask`] edges.
    Task(TaskData),
    /// A paragraph-sized section of a note. Connected to its owning
    /// note (or nearest heading) via [`EdgeKind::OwnsParagraph`] and
    /// to any wiki-link targets it mentions via
    /// [`EdgeKind::ParagraphLink`].
    Paragraph(ParagraphData),
    /// An ATX heading in a note. Connected to its nearest enclosing
    /// heading (or the note) via [`EdgeKind::OwnsHeading`], and to its
    /// sub-headings and the paragraphs under it via the same family.
    /// The heading line is also the first line of the paragraph that
    /// begins at that line (Fork A2): the heading node owns the
    /// structure, the paragraph node owns the text.
    Heading(HeadingData),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteData {
    /// Vault-relative path, e.g. `Areas/finance.md`.
    pub path: PathBuf,
    /// Filename stem (no extension), used for wikilink title resolution.
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GhostData {
    /// The verbatim unresolved target string from the linker. For
    /// wikilinks this is the pre-pipe, pre-anchor target; for markdown
    /// links it's the URL-decoded vault-relative path.
    pub raw: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirData {
    /// Vault-relative directory path, e.g. `Areas/finance` (no trailing
    /// slash). Root directory uses the empty path.
    pub path: PathBuf,
    /// Last component of the directory path. Root directory uses the
    /// empty string.
    pub name: String,
}

impl DirData {
    pub fn root() -> Self {
        DirData {
            path: PathBuf::new(),
            name: String::new(),
        }
    }
}

/// Paragraph node data — a block of contiguous markdown content within
/// one note, identified by its 1-indexed line range. Created during
/// [`Graph::build`] from [`crate::markdown::extract_paragraphs`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParagraphData {
    /// Vault-relative path of the owning note.
    pub source_file: PathBuf,
    /// 1-indexed inclusive line range start within the source note.
    pub line_start: u32,
    /// 1-indexed inclusive line range end within the source note.
    pub line_end: u32,
    /// Paragraph text — lines joined with `\n`, no trailing newline.
    pub text: String,
}

/// Heading node data — an ATX heading line in a note. Created during
/// [`Graph::build`] from [`crate::markdown::extract_headings`]. The
/// heading line is also the start of the paragraph that begins at that
/// line (Fork A2), so `ParagraphData.text` for that paragraph includes
/// the heading line verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadingData {
    /// Vault-relative path of the owning note.
    pub source_file: PathBuf,
    /// 1-indexed line number of the heading within the source note.
    pub line: u32,
    /// ATX level (1..=6).
    pub level: u8,
    /// Heading text with leading `#`s, trailing `#`s, and surrounding
    /// whitespace stripped (matches [`crate::markdown::Heading::text`]).
    pub text: String,
}

/// Task node data — denormalized from [`crate::task::Task`] for graph queries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskData {
    /// Task description text.
    pub description: String,
    /// Task status as string for DSL evaluation (e.g., "Open", "Done").
    pub status: String,
    /// Priority level, if set.
    pub priority: Option<String>,
    /// Due date as YYYY-MM-DD string, if set.
    pub due: Option<String>,
    /// Scheduled date as YYYY-MM-DD string, if set.
    pub scheduled: Option<String>,
    /// Created date as YYYY-MM-DD string, if set.
    pub created: Option<String>,
    /// Start date as YYYY-MM-DD string, if set.
    pub start: Option<String>,
    /// Completed (`done`) date as YYYY-MM-DD string, if set.
    pub completed: Option<String>,
    /// Tags extracted from the task description.
    pub tags: Vec<String>,
    /// Source file (vault-relative path) where the task appears.
    pub source_file: PathBuf,
    /// 1-indexed line number within the source file.
    pub source_line: usize,
}

/// Per-edge payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EdgeKind {
    /// One link occurrence anywhere in a note, from the note node.
    /// Target is the resolved note (or ghost); anchors are retained as
    /// metadata but note-level edges never target a heading.
    NoteLink(LinkEdge),
    /// One link occurrence on a heading line, from the heading node.
    /// Target may be a heading node (resolvable anchor) or note/ghost.
    HeadingLink(LinkEdge),
    /// One link occurrence in a paragraph body (including a heading
    /// line, since it begins a paragraph per Fork A2), from the
    /// paragraph node. Target may be a heading node (resolvable anchor)
    /// or note/ghost.
    ParagraphLink(LinkEdge),
    /// A directory contains a child node (note or subdirectory). Unit
    /// variant — there is no textual link to rewrite.
    Contains,
    /// A note has a task as a child. Edge from note node to task node.
    HasTask,
    /// A task has a subtask, established by indentation in the source
    /// markdown. Edge from the parent task node to the child task node.
    /// Always intra-file. Unit variant — no textual link to rewrite.
    Subtask,
    /// A note links to one or more notes contained in a directory.
    /// Derived edge — one per unique (source-note, target-directory) pair
    /// from resolved link edges (NoteLink/HeadingLink/ParagraphLink).
    /// Unit variant (no LinkEdge data).
    LinksInto,
    /// A note owns a paragraph node, or a heading owns a paragraph
    /// under it. Edge from note/heading → paragraph. Exclusive: each
    /// paragraph has exactly one owner (its nearest enclosing heading,
    /// or the note if there is none).
    OwnsParagraph,
    /// A note or heading owns a heading. Edge from note/heading →
    /// heading. Models the heading section tree: a heading at level `L`
    /// is owned by the nearest heading of level `< L`, or by the note.
    OwnsHeading,
}

impl EdgeKind {
    /// The `LinkEdge` payload if this edge is one of the three link kinds,
    /// else `None`.
    pub fn link(&self) -> Option<&LinkEdge> {
        match self {
            EdgeKind::NoteLink(e) | EdgeKind::HeadingLink(e) | EdgeKind::ParagraphLink(e) => {
                Some(e)
            }
            EdgeKind::Contains
            | EdgeKind::HasTask
            | EdgeKind::Subtask
            | EdgeKind::LinksInto
            | EdgeKind::OwnsParagraph
            | EdgeKind::OwnsHeading => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkForm {
    WikiLink,
    MdLink,
}

/// Where a link points. Both variants name a [`NoteId`] — `Resolved`
/// names a `Note` node, `Unresolved` names a `Ghost` node. Carrying the
/// id in both lets callers traverse uniformly via
/// [`Graph::outgoing`] / [`Graph::incoming`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkTarget {
    Resolved(NoteId),
    Unresolved(NoteId),
}

impl LinkTarget {
    pub fn note_id(self) -> NoteId {
        match self {
            LinkTarget::Resolved(id) | LinkTarget::Unresolved(id) => id,
        }
    }
    pub fn is_resolved(self) -> bool {
        matches!(self, LinkTarget::Resolved(_))
    }
}

/// Per-occurrence link record stored on each link edge (NoteLink,
/// HeadingLink, ParagraphLink).
///
/// `byte_range` indexes into the **source file's content at parse time**
/// — re-parse the file (via [`Graph::refresh_note`]) before relying on it
/// after any edit to that file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkEdge {
    pub form: LinkForm,
    /// `true` for `!`-prefixed transclusions (`![[...]]`, `![...](...)`).
    /// Embed-ness is a property of the link occurrence, not a separate
    /// edge kind, so all three link levels treat embeds uniformly.
    pub is_embed: bool,
    /// Byte range in the source file's content.
    pub byte_range: std::ops::Range<usize>,
    /// 1-indexed source line number.
    pub line: usize,
    /// Verbatim source token, e.g. `"[[Foo|alias]]"` or `"[Foo](foo.md)"`.
    pub raw_text: String,
    /// Pre-pipe, pre-anchor target text. For wikilinks, the raw target;
    /// for markdown links, the URL-decoded href (still with `.md` if
    /// present).
    pub target_text: String,
    /// Post-`#` heading anchor, if any.
    pub anchor: Option<String>,
    /// Post-`|` alias for wikilinks, or the bracketed text for markdown
    /// links. `None` when there's no display override.
    pub display: Option<String>,
}

/// Result of the parallel parse phase of [`Graph::build`]: one per
/// markdown file, bundling the vault-relative path, raw content, and the
/// extracted links, paragraphs, and headings. Struct (not a tuple) so
/// the field reads stay self-documenting and clippy's
/// `type_complexity` lint stays quiet.
pub(crate) struct ParsedFile {
    pub rel: PathBuf,
    pub links: Vec<parser::RawLink>,
    pub paragraphs: Vec<crate::markdown::Paragraph>,
    pub headings: Vec<crate::markdown::Heading>,
}

/// In-memory graph of notes and the links between them.
///
/// Built up-front by [`Graph::build`] (parallel scan). Mutated
/// incrementally by [`Graph::refresh_note`] when one file changes.
/// Read via [`Graph::incoming`] / [`Graph::outgoing`] and the lookup
/// methods.
#[derive(Debug)]
pub struct Graph {
    g: StableDiGraph<NodeKind, EdgeKind>,
    /// Vault-relative path → note node. Path canonicalization joins
    /// components with `/` so the same key shape is produced on Windows.
    path_index: HashMap<PathBuf, NoteId>,
    /// Filename stem → all note nodes with that stem. Multi because
    /// titles aren't unique across a vault.
    title_index: HashMap<String, Vec<NoteId>>,
    /// Unresolved-target string → ghost node. Shared across all linkers
    /// so removing one linker doesn't necessarily orphan the ghost.
    ghost_index: HashMap<String, NoteId>,
    /// (source_file, source_line) → task node. Used for task deduplication
    /// and lookup. The source_file is a vault-relative PathBuf.
    task_index: HashMap<(PathBuf, usize), NoteId>,
    /// (source_file, line_start) → paragraph node. Each paragraph is
    /// uniquely identified by its owning note's path and 1-indexed first
    /// line. Populated during `Graph::build` / refreshed by
    /// `Graph::refresh_note`.
    paragraph_index: HashMap<(PathBuf, u32), NoteId>,
    /// (source_file, line) → heading node. Each heading is uniquely
    /// identified by its owning note's path and 1-indexed heading line.
    /// Populated during `Graph::build` / refreshed by `Graph::refresh_note`.
    heading_index: HashMap<(PathBuf, u32), NoteId>,
}

impl Graph {
    /// Build the graph from every markdown file in the vault.
    ///
    /// Files are read and link-parsed in parallel; node insertion and
    /// edge resolution happen on the main thread to keep the side-tables
    /// consistent. Honors the same ignore rules as [`Vault::scan`]
    /// (`.obsidian/`, `.git/`, `attachments/`, `.gitignore`,
    /// `[ignored_paths]`).
    ///
    /// Task nodes are created from `scan.tasks` after the note nodes,
    /// and `HasTask` edges connect notes to their tasks.
    pub fn build(vault: &Vault, scan: &Scan) -> Result<Graph> {
        let files = vault.markdown_files();

        // Parse phase (parallel): read each file, extract raw links,
        // paragraph ranges, and headings in the same pass.
        let parsed: Vec<ParsedFile> = files
            .par_iter()
            .filter_map(|abs| {
                let rel = abs.strip_prefix(&vault.path).ok()?.to_path_buf();
                let content = std::fs::read_to_string(abs).ok()?;
                let links = parser::extract_links(&content);
                let paragraphs = crate::markdown::extract_paragraphs(&content);
                let headings = crate::markdown::extract_headings(&content);
                Some(ParsedFile {
                    rel,
                    links,
                    paragraphs,
                    headings,
                })
            })
            .collect();

        let mut graph = Graph {
            g: StableDiGraph::new(),
            path_index: HashMap::new(),
            title_index: HashMap::new(),
            ghost_index: HashMap::new(),
            task_index: HashMap::new(),
            paragraph_index: HashMap::new(),
            heading_index: HashMap::new(),
        };

        // Insert all note nodes first so resolution can see the full
        // path/title indexes for any cross-reference.
        for pf in &parsed {
            graph.insert_note_node(pf.rel.clone());
        }

        // Insert directory nodes (root + every directory the vault
        // walk yields + every parent directory of a note as a defensive
        // union, so freshly created dirs not yet on disk at walk time
        // still get nodes).
        let dirs = vault.directories();
        graph.insert_directory_nodes(&parsed, &dirs);

        // Insert contains edges from each directory to its immediate
        // children (subdirectories and notes).
        graph.insert_contains_edges();

        // Now resolve and insert link edges.
        for pf in &parsed {
            let src = *graph
                .path_index
                .get(&pf.rel)
                .expect("note node was just inserted");
            graph.insert_edges_for(src, &pf.rel, &pf.links);
        }

        // Insert heading nodes + OwnsHeading edges (heading-stack
        // algorithm). Done after note-link insertion so the path/title
        // indexes are populated, and before paragraphs so paragraph
        // ownership can resolve against the heading stack.
        for pf in &parsed {
            let src = *graph
                .path_index
                .get(&pf.rel)
                .expect("note node was just inserted");
            graph.insert_heading_nodes_for(src, &pf.rel, &pf.headings);
        }

        // Insert paragraph nodes, OwnsParagraph edges (nearest-container),
        // and ParagraphLink edges. Done after heading insertion so the
        // heading stack is available for paragraph ownership.
        for pf in &parsed {
            let src = *graph
                .path_index
                .get(&pf.rel)
                .expect("note node was just inserted");
            graph.insert_paragraph_nodes_for(src, &pf.rel, &pf.paragraphs, &pf.headings, &pf.links);
        }

        // Insert task nodes from scan data.
        for task in &scan.tasks {
            graph.insert_task_node(task);
        }

        // Create HasTask edges from notes to their top-level tasks only.
        // Subtasks are reachable via the Subtask edge chain.
        graph.insert_hastask_edges(&scan.tasks);

        // Create Subtask edges from parent tasks to their subtasks.
        graph.insert_subtask_edges(&scan.tasks);

        // Create LinksInto edges from notes to directories they link into.
        graph.insert_links_into_edges();

        Ok(graph)
    }

    /// Re-parse one file. Removes its outgoing edges (and any orphaned
    /// ghost nodes), re-extracts links from the file's current content,
    /// and inserts new edges.
    ///
    /// Incoming edges to this note are untouched — they belong to other
    /// notes' outgoing sets. If the file isn't in the graph yet (a new
    /// note), it's inserted.
    ///
    /// `path` may be absolute or already vault-relative. Both
    /// `vault_root` and absolute paths are canonicalized before
    /// `strip_prefix` so refresh works on systems where the temp dir
    /// or vault root sits behind a symlink (e.g. macOS `/tmp` →
    /// `/private/tmp`).
    pub fn refresh_note(&mut self, vault_root: &Path, path: &Path) -> Result<()> {
        let abs = if path.is_absolute() {
            path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
        } else {
            let joined = vault_root.join(path);
            joined.canonicalize().unwrap_or(joined)
        };
        let root = vault_root
            .canonicalize()
            .unwrap_or_else(|_| vault_root.to_path_buf());
        let rel = abs.strip_prefix(&root).unwrap_or(&abs).to_path_buf();

        let content = std::fs::read_to_string(&abs).map_err(|e| crate::error::Error::Io {
            path: abs.clone(),
            source: e,
        })?;
        let links = parser::extract_links(&content);
        let paragraphs = crate::markdown::extract_paragraphs(&content);
        let headings = crate::markdown::extract_headings(&content);

        let normalized = normalize_path(&rel);
        let src = match self.path_index.get(&normalized) {
            Some(id) => *id,
            None => self.insert_note_node(rel.clone()),
        };

        self.ensure_dir_chain_for(src, &rel);
        self.remove_paragraph_nodes(src);
        self.remove_heading_nodes(src);
        self.remove_outgoing_edges(src);
        self.insert_edges_for(src, &rel, &links);
        self.insert_links_into_for(src);
        self.insert_heading_nodes_for(src, &rel, &headings);
        self.insert_paragraph_nodes_for(src, &rel, &paragraphs, &headings, &links);
        Ok(())
    }

    /// Look up any node (note, directory, or ghost — though ghosts
    /// aren't stored by path) by vault-relative path.
    pub fn node_by_path(&self, p: &Path) -> Option<NoteId> {
        self.path_index.get(&normalize_path(p)).copied()
    }

    /// Look up a note backing a vault-relative path. Excludes directory
    /// nodes (use [`Graph::node_by_path`] for those).
    pub fn note_by_path(&self, p: &Path) -> Option<NoteId> {
        let id = self.node_by_path(p)?;
        matches!(self.node(id), NodeKind::Note(_)).then_some(id)
    }

    /// All notes whose filename stem equals `t`. May be empty, one, or
    /// many — titles aren't unique.
    pub fn note_by_title(&self, t: &str) -> &[NoteId] {
        self.title_index.get(t).map(Vec::as_slice).unwrap_or(&[])
    }

    /// The shared ghost node for an unresolved target string, if one
    /// has been materialized.
    pub fn ghost_by_raw(&self, raw: &str) -> Option<NoteId> {
        self.ghost_index.get(raw).copied()
    }

    /// The paragraph node at `(path, line_start)`, if any. `path` is
    /// vault-relative; `line_start` is 1-indexed.
    pub fn paragraph_by_loc(&self, path: &Path, line_start: u32) -> Option<NoteId> {
        self.paragraph_index
            .get(&(normalize_path(path), line_start))
            .copied()
    }

    /// The heading node at `(path, line)`, if any. `path` is
    /// vault-relative; `line` is 1-indexed.
    pub fn heading_by_loc(&self, path: &Path, line: u32) -> Option<NoteId> {
        self.heading_index
            .get(&(normalize_path(path), line))
            .copied()
    }

    /// The task node at `(source_file, source_line)`, if any.
    /// `source_file` is vault-relative; `source_line` is 1-indexed.
    pub fn task_by_loc(&self, source_file: &Path, source_line: usize) -> Option<NoteId> {
        self.task_index
            .get(&(normalize_path(source_file), source_line))
            .copied()
    }

    /// A build-independent identity for a node. Two `Graph`s built from
    /// the same on-disk state produce equal keys for the same nodes;
    /// keys survive arbitrary [`Graph::build`] calls as long as the
    /// underlying node still exists by path/raw/loc. Use with
    /// [`Graph::id_for_key`] to re-resolve cross-build.
    pub fn stable_key(&self, id: NoteId) -> NodeKey {
        match self.node(id) {
            NodeKind::Note(n) => NodeKey::Note(n.path.clone()),
            NodeKind::Directory(d) => NodeKey::Directory(d.path.clone()),
            NodeKind::Ghost(g) => NodeKey::Ghost(g.raw.clone()),
            NodeKind::Task(t) => NodeKey::Task(t.source_file.clone(), t.source_line),
            NodeKind::Paragraph(p) => NodeKey::Paragraph(p.source_file.clone(), p.line_start),
            NodeKind::Heading(h) => NodeKey::Heading(h.source_file.clone(), h.line),
        }
    }

    /// Resolve a stable key against this graph. Returns `None` if the
    /// underlying node no longer exists.
    pub fn id_for_key(&self, key: &NodeKey) -> Option<NoteId> {
        match key {
            NodeKey::Note(p) => {
                let id = self.node_by_path(p)?;
                matches!(self.node(id), NodeKind::Note(_)).then_some(id)
            }
            NodeKey::Directory(p) => {
                let id = self.node_by_path(p)?;
                matches!(self.node(id), NodeKind::Directory(_)).then_some(id)
            }
            NodeKey::Ghost(raw) => self.ghost_by_raw(raw),
            NodeKey::Task(path, line) => self.task_by_loc(path, *line),
            NodeKey::Paragraph(path, line_start) => self.paragraph_by_loc(path, *line_start),
            NodeKey::Heading(path, line) => self.heading_by_loc(path, *line),
        }
    }

    /// The kind of node at `id`. Panics on a stale id (one whose node
    /// was removed) — in practice ids returned from this `Graph` are
    /// always live.
    pub fn node(&self, id: NoteId) -> &NodeKind {
        &self.g[id.0]
    }

    /// All paragraphs transitively owned by `note_id`: direct
    /// `OwnsParagraph` children of the note plus `OwnsParagraph`
    /// children of every transitively-`OwnsHeading`-descendant heading.
    /// Replaces the pre-refactor flat
    /// `outgoing(note).filter(OwnsParagraph)` walk, which only returned
    /// heading-less paragraphs under nearest-container ownership.
    pub fn note_paragraphs(&self, note_id: NoteId) -> Vec<NoteId> {
        let mut out = Vec::new();
        let mut work = vec![note_id];
        while let Some(node) = work.pop() {
            for e in self.g.edges_directed(node.0, Direction::Outgoing) {
                match e.weight() {
                    EdgeKind::OwnsParagraph => out.push(NoteId(e.target())),
                    EdgeKind::OwnsHeading => work.push(NoteId(e.target())),
                    _ => {}
                }
            }
        }
        out
    }

    /// The note's direct `OwnsHeading` children (its top-level
    /// headings). For the full heading subtree use [`all_headings`].
    pub fn note_headings(&self, note_id: NoteId) -> Vec<NoteId> {
        self.g
            .edges_directed(note_id.0, Direction::Outgoing)
            .filter(|e| matches!(e.weight(), EdgeKind::OwnsHeading))
            .map(|e| NoteId(e.target()))
            .collect()
    }

    /// The full heading subtree under `note_id` (direct + transitively
    /// nested headings).
    pub fn all_headings(&self, note_id: NoteId) -> Vec<NoteId> {
        let mut out = Vec::new();
        let mut work = self.note_headings(note_id);
        while let Some(h) = work.pop() {
            out.push(h);
            for e in self.g.edges_directed(h.0, Direction::Outgoing) {
                if matches!(e.weight(), EdgeKind::OwnsHeading) {
                    work.push(NoteId(e.target()));
                }
            }
        }
        out
    }

    /// Resolve an anchor (`#heading` text) against a note's headings.
    /// Returns the heading node whose normalized `text` matches `anchor`
    /// (case-insensitive, whitespace-collapsed, trailing `#`s stripped),
    /// searching the note's full heading subtree. Returns `None` if no
    /// heading matches. Used by the build phase to target `HeadingLink`/
    /// `ParagraphLink` edges at heading nodes when an anchor resolves.
    pub fn resolve_anchor(&self, note_id: NoteId, anchor: &str) -> Option<NoteId> {
        let needle = normalize_anchor(anchor);
        if needle.is_empty() {
            return None;
        }
        self.all_headings(note_id).into_iter().find(|h_id| {
            matches!(self.node(*h_id), NodeKind::Heading(h) if normalize_anchor(&h.text) == needle)
        })
    }

    /// Resolve a link target node id to its note-level identity: for a
    /// `Heading`, walk up `OwnsHeading` to the owning `Note`; for a `Note`
    /// or `Ghost`, return as-is. Returns `None` for other node kinds.
    /// Used by consumers that treat any link (including anchored links to
    /// a heading) as a mention of the underlying note/ghost.
    pub fn link_target_note(&self, id: NoteId) -> Option<NoteId> {
        match self.node(id) {
            NodeKind::Note(_) | NodeKind::Ghost(_) => Some(id),
            NodeKind::Heading(_) => {
                // Walk up OwnsHeading until we reach a Note.
                let mut cur = id;
                loop {
                    let parent = self
                        .g
                        .edges_directed(cur.0, Direction::Incoming)
                        .find(|e| matches!(e.weight(), EdgeKind::OwnsHeading))
                        .map(|e| NoteId(e.source()));
                    match parent {
                        Some(p) => {
                            if matches!(self.node(p), NodeKind::Note(_)) {
                                return Some(p);
                            }
                            cur = p;
                        }
                        None => return None,
                    }
                }
            }
            _ => None,
        }
    }

    /// Every incoming link edge (at all three levels: NoteLink,
    /// HeadingLink, ParagraphLink) whose target is `note_id` OR any of
    /// the note's transitively-owned headings. This is the canonical
    /// "any `[[Foo…]]` mentions note Foo" traversal — anchored links that
    /// resolved to a heading still count as mentions of the heading's
    /// owning note. Returns `(source_node_id, LinkEdge)` pairs.
    pub fn mentions_of(&self, note_id: NoteId) -> Vec<(NoteId, LinkEdge)> {
        let mut targets = self.all_headings(note_id);
        targets.push(note_id);
        let target_set: HashSet<NoteId> = targets.into_iter().collect();
        let mut out = Vec::new();
        for src in self.g.node_indices() {
            for e in self.g.edges_directed(src, Direction::Outgoing) {
                let dst = NoteId(e.target());
                if !target_set.contains(&dst) {
                    continue;
                }
                if let Some(l) = e.weight().link() {
                    out.push((NoteId(src), l.clone()));
                }
            }
        }
        out
    }

    /// All nodes in the graph in arbitrary order.
    pub fn nodes(&self) -> impl Iterator<Item = (NoteId, &NodeKind)> {
        self.g
            .node_indices()
            .map(move |idx| (NoteId(idx), &self.g[idx]))
    }

    /// Edges where `id` is the source. Each yielded tuple is
    /// `(destination, edge)`. The destination may be a Note or a Ghost;
    /// callers check via [`Graph::node`].
    pub fn outgoing(&self, id: NoteId) -> impl Iterator<Item = (NoteId, &EdgeKind)> {
        self.g
            .edges_directed(id.0, Direction::Outgoing)
            .map(|e| (NoteId(e.target()), e.weight()))
    }

    /// Edges where `id` is the destination. Each yielded tuple is
    /// `(source, edge)`. Backlinks query.
    pub fn incoming(&self, id: NoteId) -> impl Iterator<Item = (NoteId, &EdgeKind)> {
        self.g
            .edges_directed(id.0, Direction::Incoming)
            .map(|e| (NoteId(e.source()), e.weight()))
    }

    // ── internals ──────────────────────────────────────────────────────

    fn insert_note_node(&mut self, rel: PathBuf) -> NoteId {
        let normalized = normalize_path(&rel);
        if let Some(id) = self.path_index.get(&normalized) {
            return *id;
        }
        let title = title_of(&rel);
        let kind = NodeKind::Note(NoteData {
            path: normalized.clone(),
            title: title.clone(),
        });
        let idx = self.g.add_node(kind);
        let id = NoteId(idx);
        self.path_index.insert(normalized, id);
        self.title_index.entry(title).or_default().push(id);
        id
    }

    /// Get-or-create the shared ghost node for `raw`.
    fn intern_ghost(&mut self, raw: &str) -> NoteId {
        if let Some(id) = self.ghost_index.get(raw) {
            return *id;
        }
        let idx = self.g.add_node(NodeKind::Ghost(GhostData {
            raw: raw.to_string(),
        }));
        let id = NoteId(idx);
        self.ghost_index.insert(raw.to_string(), id);
        id
    }

    fn remove_outgoing_edges(&mut self, src: NoteId) {
        let edge_ids: Vec<_> = self
            .g
            .edges_directed(src.0, Direction::Outgoing)
            .map(|e| e.id())
            .collect();
        // Capture the ghost neighbors before we drop the edges so we can
        // garbage-collect any that lose their last incoming edge.
        let ghost_neighbors: Vec<NoteId> = edge_ids
            .iter()
            .filter_map(|eid| {
                let (_, dst) = self.g.edge_endpoints(*eid)?;
                matches!(self.g[dst], NodeKind::Ghost(_)).then_some(NoteId(dst))
            })
            .collect();
        for eid in edge_ids {
            self.g.remove_edge(eid);
        }
        for ghost in ghost_neighbors {
            if self
                .g
                .edges_directed(ghost.0, Direction::Incoming)
                .next()
                .is_none()
            {
                if let NodeKind::Ghost(GhostData { raw }) = &self.g[ghost.0] {
                    self.ghost_index.remove(raw);
                }
                self.g.remove_node(ghost.0);
            }
        }
    }

    fn insert_edges_for(&mut self, src: NoteId, src_rel: &Path, links: &[parser::RawLink]) {
        for raw in links {
            let target = match raw.form {
                LinkForm::WikiLink => resolve::resolve_wiki(&raw.target_text, self),
                LinkForm::MdLink => resolve::resolve_md(&raw.target_text, src_rel, self),
            };
            let dst = match target {
                resolve::Resolution::Resolved(id) => id,
                resolve::Resolution::Unresolved(key) => self.intern_ghost(&key),
                resolve::Resolution::NotALink => continue,
            };
            let edge = LinkEdge {
                form: raw.form,
                is_embed: raw.is_embed,
                byte_range: raw.byte_range.clone(),
                line: raw.line,
                raw_text: raw.raw_text.clone(),
                target_text: raw.target_text.clone(),
                anchor: raw.anchor.clone(),
                display: raw.display.clone(),
            };
            // Note-level link: one per occurrence, targeting the note
            // (or ghost). Anchors are retained as metadata but note-level
            // edges never target a heading (D5).
            self.g.add_edge(src.0, dst.0, EdgeKind::NoteLink(edge));
        }
    }

    fn insert_directory_nodes(&mut self, parsed: &[ParsedFile], vault_dirs: &[PathBuf]) {
        let mut dir_paths: BTreeSet<PathBuf> = BTreeSet::new();
        // From the filesystem walk: every dir, including empty ones and
        // dirs whose only content is non-markdown (attachments not
        // covered by the default-ignore rule).
        for dir in vault_dirs {
            dir_paths.insert(normalize_path(dir));
        }
        // From note ancestors: covers dirs that the walk missed because
        // they came into existence after the walk started.
        for pf in parsed {
            let normalized = normalize_path(&pf.rel);
            let mut current = normalized.parent();
            while let Some(parent) = current {
                if !parent.as_os_str().is_empty() {
                    dir_paths.insert(normalize_path(parent));
                }
                current = parent.parent();
            }
        }

        self.insert_directory_node(PathBuf::new(), String::new());

        for dir_path in &dir_paths {
            let name = dir_path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            self.insert_directory_node(dir_path.clone(), name);
        }
    }

    fn insert_directory_node(&mut self, path: PathBuf, name: String) -> NoteId {
        let normalized = normalize_path(&path);
        if let Some(id) = self.path_index.get(&normalized) {
            return *id;
        }
        let kind = NodeKind::Directory(DirData {
            path: normalized.clone(),
            name,
        });
        let idx = self.g.add_node(kind);
        let id = NoteId(idx);
        self.path_index.insert(normalized, id);
        id
    }

    /// Ensure every ancestor directory of `note_rel` exists as a node
    /// and that the chain of `Contains` edges from root → … → parent →
    /// note is wired up. Idempotent; cheap when the chain is already
    /// present. Called from `refresh_note` so a new file in a brand-new
    /// directory still gets its dir nodes and contains edges, since the
    /// build-time walks don't re-run.
    fn ensure_dir_chain_for(&mut self, note_id: NoteId, note_rel: &Path) {
        let normalized = normalize_path(note_rel);
        let parent = normalized
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();

        let mut chain: Vec<PathBuf> = vec![PathBuf::new()];
        let mut acc = PathBuf::new();
        for comp in parent.components() {
            if let std::path::Component::Normal(s) = comp {
                acc.push(s);
                chain.push(acc.clone());
            }
        }

        let mut parent_id = self.insert_directory_node(PathBuf::new(), String::new());
        for dir_path in chain.iter().skip(1) {
            let name = dir_path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let dir_id = self.insert_directory_node(dir_path.clone(), name);
            self.ensure_contains_edge(parent_id, dir_id);
            parent_id = dir_id;
        }
        self.ensure_contains_edge(parent_id, note_id);
    }

    fn ensure_contains_edge(&mut self, parent: NoteId, child: NoteId) {
        let exists = self
            .g
            .edges_directed(parent.0, Direction::Outgoing)
            .any(|e| e.target() == child.0 && matches!(e.weight(), EdgeKind::Contains));
        if !exists {
            self.g.add_edge(parent.0, child.0, EdgeKind::Contains);
        }
    }

    fn insert_contains_edges(&mut self) {
        let dirs: Vec<(NoteId, PathBuf)> = self
            .g
            .node_indices()
            .filter_map(|idx| {
                if let NodeKind::Directory(data) = &self.g[idx] {
                    Some((NoteId(idx), data.path.clone()))
                } else {
                    None
                }
            })
            .collect();

        let children: Vec<(NoteId, PathBuf)> = self
            .g
            .node_indices()
            .filter_map(|idx| match &self.g[idx] {
                NodeKind::Note(data) => Some((NoteId(idx), data.path.clone())),
                NodeKind::Directory(data) => {
                    if data.path.as_os_str().is_empty() {
                        None
                    } else {
                        Some((NoteId(idx), data.path.clone()))
                    }
                }
                _ => None,
            })
            .collect();

        for (parent_id, parent_path) in &dirs {
            for (child_id, child_path) in &children {
                if is_immediate_child(parent_path, child_path) {
                    self.g.add_edge(parent_id.0, child_id.0, EdgeKind::Contains);
                }
            }
        }
    }

    /// Insert a task node from a Task struct. Deduplicates by (source_file, source_line).
    /// Returns the NoteId of the task node (existing or newly created).
    fn insert_task_node(&mut self, task: &Task) -> NoteId {
        let key = (task.source_file.clone(), task.source_line);
        if let Some(id) = self.task_index.get(&key) {
            return *id;
        }

        let task_data = TaskData {
            description: task.description.clone(),
            status: task.status.as_str().to_string(),
            priority: task.priority.map(|p| p.as_str().to_string()),
            due: task.due.map(|d| d.to_string()),
            scheduled: task.scheduled.map(|s| s.to_string()),
            created: task.created.map(|d| d.to_string()),
            start: task.start.map(|d| d.to_string()),
            completed: task.done.map(|d| d.to_string()),
            tags: task.tags.clone(),
            source_file: task.source_file.clone(),
            source_line: task.source_line,
        };

        let idx = self.g.add_node(NodeKind::Task(task_data));
        let id = NoteId(idx);
        self.task_index.insert(key, id);
        id
    }

    /// Create HasTask edges from each note node to its **top-level** task
    /// nodes only. Subtasks (tasks whose `parent` is `Some`) are reachable
    /// from their note exclusively via the chain
    /// `Note →[HasTask]→ top-level task →[Subtask]→ …`; they receive no
    /// direct HasTask edge, which keeps "tasks of note N" a deduped tree
    /// by construction. Matches tasks to notes by
    /// `TaskData.source_file == NoteData.path` using the already-built
    /// `path_index` for O(N) lookup.
    fn insert_hastask_edges(&mut self, tasks: &[Task]) {
        for task in tasks {
            if task.parent.is_some() {
                continue;
            }
            let Some(&task_id) = self
                .task_index
                .get(&(task.source_file.clone(), task.source_line))
            else {
                continue;
            };
            if let Some(&note_id) = self.path_index.get(&task.source_file) {
                if matches!(self.g[note_id.0], NodeKind::Note(_)) {
                    self.g.add_edge(note_id.0, task_id.0, EdgeKind::HasTask);
                }
            }
        }
    }

    /// Create Subtask edges from each parent task to its direct subtasks.
    /// The parent relationship is the `Task.parent` line pointer resolved at
    /// scan time; both endpoints live in the same file, so we look the parent
    /// up by `(source_file, parent_line)` in the task index.
    fn insert_subtask_edges(&mut self, tasks: &[Task]) {
        for task in tasks {
            let Some(parent_line) = task.parent else {
                continue;
            };
            let child = match self
                .task_index
                .get(&(task.source_file.clone(), task.source_line))
            {
                Some(&id) => id,
                None => continue,
            };
            if let Some(&parent) = self
                .task_index
                .get(&(task.source_file.clone(), parent_line))
            {
                self.g.add_edge(parent.0, child.0, EdgeKind::Subtask);
            }
        }
    }

    /// Create LinksInto edges from each note to every directory that
    /// contains at least one note it links to (via Link or Embed).
    /// Deduplicates: at most one edge per (source, directory) pair.
    fn insert_links_into_edges(&mut self) {
        let note_ids: Vec<NoteId> = self
            .g
            .node_indices()
            .filter(|idx| matches!(self.g[*idx], NodeKind::Note(_)))
            .map(NoteId)
            .collect();
        for src in note_ids {
            self.insert_links_into_for(src);
        }
    }

    /// Insert heading nodes and `OwnsHeading` edges for one note using
    /// the heading-stack algorithm. A heading at level `L` is owned by
    /// the nearest enclosing heading of level `< L`, or by the note if
    /// there is no such heading. Populates `heading_index`.
    fn insert_heading_nodes_for(
        &mut self,
        note_id: NoteId,
        note_rel: &Path,
        headings: &[crate::markdown::Heading],
    ) {
        let note_rel_norm = normalize_path(note_rel);
        // Stack of (level, heading NoteId) currently open.
        let mut stack: Vec<(u8, NoteId)> = Vec::new();
        for heading in headings {
            // Pop every heading with level >= this one (its section
            // has closed).
            while let Some(&(top_level, _)) = stack.last() {
                if top_level >= heading.level {
                    stack.pop();
                } else {
                    break;
                }
            }
            let parent = stack.last().map(|&(_, id)| id).unwrap_or(note_id);
            let data = HeadingData {
                source_file: note_rel_norm.clone(),
                line: heading.line as u32,
                level: heading.level,
                text: heading.text.clone(),
            };
            let idx = self.g.add_node(NodeKind::Heading(data));
            let h_id = NoteId(idx);
            self.heading_index
                .insert((note_rel_norm.clone(), heading.line as u32), h_id);
            self.g.add_edge(parent.0, h_id.0, EdgeKind::OwnsHeading);
            stack.push((heading.level, h_id));
        }
    }

    /// Remove all heading nodes owned (directly or transitively) by
    /// `note_id`'s heading subtree. Walks `OwnsHeading` from the note,
    /// removes each heading node (with its edges) and its
    /// `heading_index` entry. Orphaned ghosts (HeadingLink-only ghosts
    /// that lose their last incoming edge) are garbage-collected.
    /// Incoming edges from other notes are not touched (they belong to
    /// those notes' outgoing sets).
    fn remove_heading_nodes(&mut self, note_id: NoteId) {
        // Collect all heading descendants via OwnsHeading DFS.
        let mut heading_ids: Vec<NoteId> = Vec::new();
        let mut work: Vec<NoteId> = vec![note_id];
        while let Some(node) = work.pop() {
            for e in self.g.edges_directed(node.0, Direction::Outgoing) {
                if matches!(e.weight(), EdgeKind::OwnsHeading) {
                    let child = NoteId(e.target());
                    heading_ids.push(child);
                    work.push(child);
                }
            }
        }
        // Collect ghost neighbors of those headings before removal
        // (HeadingLink edges).
        let mut ghost_candidates: Vec<NoteId> = Vec::new();
        for h_id in &heading_ids {
            for e in self.g.edges_directed(h_id.0, Direction::Outgoing) {
                if matches!(e.weight(), EdgeKind::HeadingLink(_))
                    && matches!(self.g[e.target()], NodeKind::Ghost(_))
                {
                    ghost_candidates.push(NoteId(e.target()));
                }
            }
        }
        for h_id in &heading_ids {
            if let NodeKind::Heading(data) = &self.g[h_id.0] {
                self.heading_index
                    .remove(&(data.source_file.clone(), data.line));
            }
        }
        for h_id in heading_ids {
            self.g.remove_node(h_id.0);
        }
        // GC orphaned ghosts.
        for ghost in ghost_candidates {
            if self
                .g
                .edges_directed(ghost.0, Direction::Incoming)
                .next()
                .is_none()
            {
                if let NodeKind::Ghost(GhostData { raw }) = &self.g[ghost.0] {
                    self.ghost_index.remove(raw);
                }
                self.g.remove_node(ghost.0);
            }
        }
    }

    /// Insert paragraph nodes and their edges for one note. Creates
    /// one `NodeKind::Paragraph` per element of `paragraphs`, an
    /// `OwnsParagraph` edge from the paragraph's **nearest container**
    /// (the heading on top of the heading stack at the paragraph's start
    /// line, or the note if there is none) → paragraph, and a
    /// `ParagraphLink` edge from paragraph → each wiki-form link
    /// target it contains. Markdown-form links are ignored — only
    /// `[[...]]` and `![[...]]` produce ParagraphLink edges.
    ///
    /// Must run after `insert_heading_nodes_for` so headings exist in
    /// the graph. The heading/paragraph merged walk below enforces the
    /// build ordering invariant (Fork A2): when a heading and a
    /// paragraph share a start line, the heading is processed first so
    /// the paragraph is owned by its own heading.
    fn insert_paragraph_nodes_for(
        &mut self,
        note_id: NoteId,
        note_rel: &Path,
        paragraphs: &[crate::markdown::Paragraph],
        headings: &[crate::markdown::Heading],
        links: &[parser::RawLink],
    ) {
        let note_rel_norm = normalize_path(note_rel);
        // Stack of (level, heading NoteId) currently open. The top is
        // the nearest enclosing heading for any paragraph at or after
        // the heading's line, until a heading of equal-or-lower level
        // is pushed (which pops it).
        let mut stack: Vec<(u8, NoteId)> = Vec::new();
        let mut h_idx = 0usize;

        for paragraph in paragraphs {
            // Advance through every heading whose line <= this
            // paragraph's start line. A heading exactly at the
            // paragraph's start line is pushed before the paragraph is
            // assigned an owner, so it owns the paragraph (Fork A2).
            while h_idx < headings.len() && headings[h_idx].line as u32 <= paragraph.line_start {
                let heading = &headings[h_idx];
                // Pop sections that have closed: any heading whose
                // level >= this heading's level.
                while let Some(&(top_level, _)) = stack.last() {
                    if top_level >= heading.level {
                        stack.pop();
                    } else {
                        break;
                    }
                }
                // The heading node was inserted by insert_heading_nodes_for;
                // look it up by loc.
                if let Some(&h_id) = self
                    .heading_index
                    .get(&(note_rel_norm.clone(), heading.line as u32))
                {
                    stack.push((heading.level, h_id));
                }
                h_idx += 1;
            }
            let owner = stack.last().map(|&(_, id)| id).unwrap_or(note_id);

            let data = ParagraphData {
                source_file: note_rel_norm.clone(),
                line_start: paragraph.line_start,
                line_end: paragraph.line_end,
                text: paragraph.text.clone(),
            };
            let idx = self.g.add_node(NodeKind::Paragraph(data));
            let p_id = NoteId(idx);
            self.paragraph_index
                .insert((note_rel_norm.clone(), paragraph.line_start), p_id);
            self.g.add_edge(owner.0, p_id.0, EdgeKind::OwnsParagraph);

            // Build a set of heading lines for O(1) "is this link on a
            // heading line?" checks. The heading nodes were inserted by
            // insert_heading_nodes_for and are in heading_index.
            // (Rebuilt once per paragraph from the headings slice; the
            // slice is small and this keeps the loop simple.)
            // Note: headings is already in document order from
            // extract_headings; we don't mutate it here.
            for raw in links {
                let line = raw.line as u32;
                if line < paragraph.line_start || line > paragraph.line_end {
                    continue;
                }
                // Resolve both wiki and markdown forms (unified link
                // kinds include both at all three levels).
                let target = match raw.form {
                    LinkForm::WikiLink => resolve::resolve_wiki(&raw.target_text, self),
                    LinkForm::MdLink => resolve::resolve_md(&raw.target_text, note_rel, self),
                };
                let note_dst = match target {
                    resolve::Resolution::Resolved(id) => id,
                    resolve::Resolution::Unresolved(key) => self.intern_ghost(&key),
                    resolve::Resolution::NotALink => continue,
                };
                // Container-level target (HeadingLink/ParagraphLink):
                // if the link has an anchor and resolves to a note, try
                // to resolve the anchor to a heading node in that note.
                // Falls back to the note (or ghost) if the anchor doesn't
                // resolve. NoteLink always targets note_dst (D5).
                let container_dst = (|| {
                    let anchor = raw.anchor.as_ref()?;
                    if matches!(self.node(note_dst), NodeKind::Note(_)) {
                        self.resolve_anchor(note_dst, anchor)
                    } else {
                        None
                    }
                })()
                .unwrap_or(note_dst);
                let edge = LinkEdge {
                    form: raw.form,
                    is_embed: raw.is_embed,
                    byte_range: raw.byte_range.clone(),
                    line: raw.line,
                    raw_text: raw.raw_text.clone(),
                    target_text: raw.target_text.clone(),
                    anchor: raw.anchor.clone(),
                    display: raw.display.clone(),
                };
                // Paragraph-level link: always, for every occurrence in
                // the paragraph (including heading lines, per Fork A2).
                self.g.add_edge(
                    p_id.0,
                    container_dst.0,
                    EdgeKind::ParagraphLink(edge.clone()),
                );
                // Heading-level link: when the occurrence is on a heading
                // line. A heading line begins a paragraph, so a link there
                // produces both a HeadingLink (from the heading) and a
                // ParagraphLink (from the paragraph) — the intended overlap.
                if let Some(&h_id) = self.heading_index.get(&(note_rel_norm.clone(), line)) {
                    self.g
                        .add_edge(h_id.0, container_dst.0, EdgeKind::HeadingLink(edge));
                }
            }
        }
    }

    /// Remove all paragraph nodes under `note_id` (direct + heading-owned,
    /// via [`Graph::note_paragraphs`]). Each removal takes the
    /// `OwnsParagraph` edge and any outgoing `ParagraphLink` edges with it
    /// (petgraph removes connected edges automatically). Orphaned ghosts
    /// (ParagraphLink-only ghosts that lose their last incoming edge)
    /// are garbage-collected.
    fn remove_paragraph_nodes(&mut self, note_id: NoteId) {
        // All paragraphs under the note — direct (heading-less) plus
        // those owned by any transitively-owned heading. Headings still
        // exist at this point in refresh_note, so note_paragraphs sees
        // the full set.
        let paragraph_ids = self.note_paragraphs(note_id);

        // Collect ghost neighbors of those paragraphs before removal.
        let mut ghost_candidates: Vec<NoteId> = Vec::new();
        for p_id in &paragraph_ids {
            for e in self.g.edges_directed(p_id.0, Direction::Outgoing) {
                if matches!(e.weight(), EdgeKind::ParagraphLink(_))
                    && matches!(self.g[e.target()], NodeKind::Ghost(_))
                {
                    ghost_candidates.push(NoteId(e.target()));
                }
            }
        }

        // Remove paragraph nodes (with their edges) and their index entries.
        for p_id in paragraph_ids {
            if let NodeKind::Paragraph(data) = &self.g[p_id.0] {
                self.paragraph_index
                    .remove(&(data.source_file.clone(), data.line_start));
            }
            self.g.remove_node(p_id.0);
        }

        // GC orphaned ghosts.
        for ghost in ghost_candidates {
            if self
                .g
                .edges_directed(ghost.0, Direction::Incoming)
                .next()
                .is_none()
            {
                if let NodeKind::Ghost(GhostData { raw }) = &self.g[ghost.0] {
                    self.ghost_index.remove(raw);
                }
                self.g.remove_node(ghost.0);
            }
        }
    }

    /// Insert LinksInto edges for a single source note. Removes any
    /// existing LinksInto edges from this source first (idempotent for
    /// build, essential for refresh_note where remove_outgoing_edges
    /// already dropped them).
    fn insert_links_into_for(&mut self, src: NoteId) {
        let mut seen_dirs: HashSet<NoteId> = HashSet::new();
        // Collect candidate (target, edge) pairs before iterating to
        // avoid borrow conflicts with add_edge.
        let candidates: Vec<(NoteId, EdgeKind)> = self
            .g
            .edges_directed(src.0, Direction::Outgoing)
            .map(|e| (NoteId(e.target()), e.weight().clone()))
            .collect();
        for (dst, edge) in &candidates {
            if !matches!(edge, EdgeKind::NoteLink(_)) {
                continue;
            }
            if let NodeKind::Note(note_data) = self.node(*dst) {
                let parent_path = normalize_path(note_data.path.parent().unwrap_or(Path::new("")));
                if let Some(dir_id) = self.node_by_path(&parent_path) {
                    if seen_dirs.insert(dir_id) {
                        self.g.add_edge(src.0, dir_id.0, EdgeKind::LinksInto);
                    }
                }
            }
        }
    }
}

/// Filename stem (no extension) used as the title for wikilink
/// resolution. Empty stem (e.g. `.md` with no name) becomes `""`.
pub(crate) fn title_of(rel: &Path) -> String {
    rel.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Canonicalize a vault-relative path so lookups produce consistent
/// keys regardless of platform separator.
pub(crate) fn normalize_path(p: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Normal(s) => out.push(s),
            Component::CurDir => {}
            // Parent / RootDir / Prefix shouldn't appear in vault-relative
            // paths; preserve them verbatim if they do rather than
            // silently rewriting.
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Normalize a heading anchor for matching: lowercase, collapse internal
/// whitespace runs to a single space, trim, and strip trailing `#`s.
/// Matches [`crate::markdown::Heading::text`] normalization (which already
/// strips leading `#`s + the required space and trailing `#`s/whitespace).
fn normalize_anchor(s: &str) -> String {
    let mut t = s.trim().to_string();
    // Strip trailing `#`s and any whitespace before them (closing hashes).
    while t.ends_with('#') {
        t.pop();
    }
    let t = t.trim();
    let mut out = String::with_capacity(t.len());
    let mut prev_ws = false;
    for c in t.chars() {
        if c.is_whitespace() {
            if !prev_ws {
                out.push(' ');
            }
            prev_ws = true;
        } else {
            out.push(c.to_ascii_lowercase());
            prev_ws = false;
        }
    }
    out
}

fn is_immediate_child(parent: &Path, child: &Path) -> bool {
    if parent.as_os_str().is_empty() {
        child.components().count() == 1
    } else {
        child
            .parent()
            .map(|p| normalize_path(p) == normalize_path(parent))
            .unwrap_or(false)
    }
}
