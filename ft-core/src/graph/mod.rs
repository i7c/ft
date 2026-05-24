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
//! [`EdgeKind::Link`] / [`EdgeKind::Embed`], but the enum shape is here so
//! later plans can add `Folder`, `Task`, `Tag`, `FrontmatterValue`, `HasTag`
//! etc. additively without rewriting the graph type.
//!
//! ## Ghost nodes
//!
//! When a wikilink or markdown link doesn't resolve to a real note, the
//! graph materializes a [`NodeKind::Ghost`] node keyed by the unresolved
//! string and points the edge at it. Multiple linkers to the same
//! unresolved target share one ghost (via `ghost_index`). This unifies
//! traversal — `incoming(ghost)` works exactly like `incoming(note)`,
//! which is what enables "rename a not-yet-created note" in session 3.

pub mod parser;
pub mod query;
pub mod rename;
pub mod resolve;

#[cfg(test)]
mod tests;

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use petgraph::stable_graph::{NodeIndex, StableDiGraph};
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use rayon::prelude::*;

use crate::error::Result;
use crate::vault::Vault;

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

/// Per-edge payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EdgeKind {
    /// `[[Foo]]`, `[[Foo|alias]]`, `[[Foo#anchor]]`, `[Foo](path.md)`, ...
    Link(LinkEdge),
    /// `![[Foo]]`, `![[image.png]]`, `![alt](path.png)` — same data shape
    /// as `Link`, distinct variant so callers can filter.
    Embed(LinkEdge),
    /// A directory contains a child node (note or subdirectory). Unit
    /// variant — there is no textual link to rewrite.
    Contains,
}

impl EdgeKind {
    pub fn link(&self) -> Option<&LinkEdge> {
        match self {
            EdgeKind::Link(e) | EdgeKind::Embed(e) => Some(e),
            EdgeKind::Contains => None,
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

/// Per-occurrence link record stored on each edge.
///
/// `byte_range` indexes into the **source file's content at parse time**
/// — re-parse the file (via [`Graph::refresh_note`]) before relying on it
/// after any edit to that file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkEdge {
    pub form: LinkForm,
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
}

impl Graph {
    /// Build the graph from every markdown file in the vault.
    ///
    /// Files are read and link-parsed in parallel; node insertion and
    /// edge resolution happen on the main thread to keep the side-tables
    /// consistent. Honors the same ignore rules as [`Vault::scan`]
    /// (`.obsidian/`, `.git/`, `attachments/`, `.gitignore`,
    /// `[ignored_paths]`).
    pub fn build(vault: &Vault) -> Result<Graph> {
        let files = vault.markdown_files();

        // Parse phase (parallel): read each file, extract raw links.
        let parsed: Vec<(PathBuf, String, Vec<parser::RawLink>)> = files
            .par_iter()
            .filter_map(|abs| {
                let rel = abs.strip_prefix(&vault.path).ok()?.to_path_buf();
                let content = std::fs::read_to_string(abs).ok()?;
                let links = parser::extract_links(&content);
                Some((rel, content, links))
            })
            .collect();

        let mut graph = Graph {
            g: StableDiGraph::new(),
            path_index: HashMap::new(),
            title_index: HashMap::new(),
            ghost_index: HashMap::new(),
        };

        // Insert all note nodes first so resolution can see the full
        // path/title indexes for any cross-reference.
        for (rel, _content, _links) in &parsed {
            graph.insert_note_node(rel.clone());
        }

        // Insert directory nodes (root + every unique parent directory
        // path across all note files).
        graph.insert_directory_nodes(&parsed);

        // Insert contains edges from each directory to its immediate
        // children (subdirectories and notes).
        graph.insert_contains_edges();

        // Now resolve and insert link edges.
        for (rel, _content, links) in &parsed {
            let src = *graph
                .path_index
                .get(rel)
                .expect("note node was just inserted");
            graph.insert_edges_for(src, rel, links);
        }

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

        let normalized = normalize_path(&rel);
        let src = match self.path_index.get(&normalized) {
            Some(id) => *id,
            None => self.insert_note_node(rel.clone()),
        };

        self.remove_outgoing_edges(src);
        self.insert_edges_for(src, &rel, &links);
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

    /// The kind of node at `id`. Panics on a stale id (one whose node
    /// was removed) — in practice ids returned from this `Graph` are
    /// always live.
    pub fn node(&self, id: NoteId) -> &NodeKind {
        &self.g[id.0]
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
                byte_range: raw.byte_range.clone(),
                line: raw.line,
                raw_text: raw.raw_text.clone(),
                target_text: raw.target_text.clone(),
                anchor: raw.anchor.clone(),
                display: raw.display.clone(),
            };
            let kind = if raw.is_embed {
                EdgeKind::Embed(edge)
            } else {
                EdgeKind::Link(edge)
            };
            self.g.add_edge(src.0, dst.0, kind);
        }
    }

    fn insert_directory_nodes(&mut self, parsed: &[(PathBuf, String, Vec<parser::RawLink>)]) {
        let mut dir_paths: BTreeSet<PathBuf> = BTreeSet::new();
        for (rel, _, _) in parsed {
            let normalized = normalize_path(rel);
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
