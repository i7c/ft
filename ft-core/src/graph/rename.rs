//! Note rename — pure planner + applier.
//!
//! [`plan_rename`] walks every incoming edge of the note being renamed
//! (which may be a real note or a ghost) and produces a [`RenamePlan`]:
//! a file-rename (when the source is a real note) plus per-linker text
//! edits that point at the new title or path. The plan is pure data —
//! [`apply_rename_plan`] is the only function that touches disk.
//!
//! ## Edit ordering
//!
//! Within a single file, multiple link occurrences turn into multiple
//! [`FileEdit`]s with overlapping-byte-space concerns. The applier
//! sorts by `byte_range.start` **descending** and applies in that
//! order, so each `replace_range` only shifts bytes we've already
//! finished with. This is the same convention LSP refactors use. The
//! invariant is non-overlap; we validate it and fail loudly on a
//! planner bug.
//!
//! ## Edit-then-rename
//!
//! When a note links to itself, the linker file *is* the file being
//! renamed. Editing first (against the file at its old path) keeps the
//! planner's byte ranges valid; the rename moves the now-correct file
//! last via `std::fs::rename`.
//!
//! ## Cross-file atomicity
//!
//! Per-file atomicity via `fs::write_atomic` is guaranteed; multi-file
//! atomicity is **not** (POSIX limitation). A crash between files
//! leaves partial state — recoverable by re-running the rename, since
//! the planner's "rewrite all linkers of <name>" is idempotent.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::fs::write_atomic;
use crate::graph::{normalize_path, EdgeKind, Graph, LinkForm, NodeKind, NoteId};

/// File system rename — `from` will be moved to `to` after every linker
/// has been updated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRename {
    pub from: PathBuf,
    pub to: PathBuf,
}

/// One byte-precise edit in one file. `byte_range` indexes the file's
/// content as it was when [`plan_rename`] ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEdit {
    /// Vault-relative path of the file to edit.
    pub path: PathBuf,
    pub byte_range: std::ops::Range<usize>,
    pub replacement: String,
}

/// Snapshot of a touched file at plan time. The applier re-stats each
/// snapshot before writing and bails if `(mtime, len)` differ — catches
/// the "user edited the file in another tool between plan and apply"
/// case. Same shape `task::ops::plan_move` uses.
#[derive(Debug, Clone)]
pub struct FileSnapshot {
    pub path: PathBuf,
    pub mtime: std::time::SystemTime,
    pub len: u64,
}

/// Pure description of a rename. Build with [`plan_rename`] or
/// [`plan_multi_rename`]; apply with [`apply_rename_plan`].
#[derive(Debug, Clone)]
pub struct RenamePlan {
    /// File-system moves. Empty for ghost-only renames or same-path
    /// no-ops. Applied after all edits.
    pub renames: Vec<FileRename>,
    /// Per-linker text edits, in arbitrary order. The applier groups by
    /// path and sorts descending before writing.
    pub edits: Vec<FileEdit>,
    /// One snapshot per touched file (each linker file plus each
    /// renamed source file if applicable). Used by the applier's
    /// freshness check.
    pub snapshots: Vec<FileSnapshot>,
}

impl RenamePlan {
    /// Number of distinct files this plan will write. (Source-renames
    /// count; pure ghost renames touch only linker files.)
    pub fn touched_files(&self) -> usize {
        let mut paths: std::collections::BTreeSet<&Path> =
            self.edits.iter().map(|e| e.path.as_path()).collect();
        for r in &self.renames {
            paths.insert(r.from.as_path());
        }
        paths.len()
    }
}

/// Build a [`RenamePlan`] for renaming `src` to `new_path`.
///
/// Convenience wrapper around [`plan_multi_rename`] for the
/// single-note case.
///
/// `new_path` is **vault-relative**. The caller (CLI / TUI) is
/// responsible for translating user input into a vault-relative path
/// (handling `mv`-style ergonomics like "bare name in same directory"
/// and `.md` auto-append).
///
/// Errors:
/// - `Error::Notes` when `new_path` already exists on disk and isn't
///   the same file as the source — refuses to clobber.
/// - `Error::Notes` when the new title would be empty (`.md` etc.).
/// - `Error::Io` when reading a touched file fails (for the snapshot).
pub fn plan_rename(
    graph: &Graph,
    vault_root: &Path,
    src: NoteId,
    new_path: &Path,
) -> Result<RenamePlan> {
    plan_multi_rename(graph, vault_root, &[(src, new_path.to_path_buf())])
}

/// Build a combined [`RenamePlan`] for renaming multiple notes at once.
///
/// Each `(NoteId, PathBuf)` pair specifies a source note (real or ghost)
/// and its new vault-relative path. All edits are computed from a single
/// graph snapshot so cross-references between the moved notes are
/// handled correctly (edit-then-rename ordering).
///
/// Same-path pairs are silently skipped. Pairs where the new file already
/// exists on disk (and is not the source file) return `Err`.
///
/// Errors:
/// - `Error::Notes` when any `new_path` already exists on disk and isn't
///   the same file as its source — refuses to clobber.
/// - `Error::Notes` when any new title would be empty.
/// - `Error::Io` when reading a touched file fails.
pub fn plan_multi_rename(
    graph: &Graph,
    vault_root: &Path,
    moves: &[(NoteId, PathBuf)],
) -> Result<RenamePlan> {
    let mut renames: Vec<FileRename> = Vec::new();
    let mut edits: Vec<FileEdit> = Vec::new();
    let mut touched_files: std::collections::BTreeSet<PathBuf> = std::collections::BTreeSet::new();

    // Build a map from old (vault-relative) path → new path for every
    // note being renamed, so that when a linker is itself being moved
    // we can compute relative URLs from its new location rather than
    // its old one.
    let mut path_map: std::collections::HashMap<PathBuf, PathBuf> =
        std::collections::HashMap::new();
    for &(src, ref new_path_raw) in moves {
        if let NodeKind::Note(n) = graph.node(src) {
            if n.path != *new_path_raw {
                path_map.insert(n.path.clone(), normalize_path(new_path_raw));
            }
        }
    }

    for &(src, ref new_path_raw) in moves {
        let new_path = normalize_path(new_path_raw);
        let new_title = new_path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .ok_or_else(|| {
                Error::Notes(format!("new path has no file stem: {}", new_path.display()))
            })?;
        if new_title.is_empty() {
            return Err(Error::Notes(format!(
                "new path has an empty filename stem: {}",
                new_path.display()
            )));
        }

        let file_rename;
        let source_rel_for_snapshot;
        match graph.node(src) {
            NodeKind::Note(data) => {
                let from = data.path.clone();
                let abs_to = vault_root.join(&new_path);
                if from != new_path && abs_to.exists() {
                    return Err(Error::Notes(format!(
                        "target already exists: {} — refusing to overwrite",
                        new_path.display()
                    )));
                }
                // Same-path skip: no rename, no edits.
                if from == new_path {
                    touched_files.insert(from.clone());
                    continue;
                } else {
                    file_rename = Some(FileRename {
                        from: from.clone(),
                        to: new_path.clone(),
                    });
                    source_rel_for_snapshot = Some(from);
                }
            }
            NodeKind::Ghost(_) => {
                file_rename = None;
                source_rel_for_snapshot = None;
            }
            NodeKind::Directory(_) => {
                return Err(Error::Notes(format!(
                    "renaming directory nodes is not yet supported: {}",
                    new_path.display()
                )));
            }
            NodeKind::Task(_) => {
                return Err(Error::Notes(format!(
                    "renaming task nodes is not supported: {}",
                    new_path.display()
                )));
            }
            NodeKind::Paragraph(_) => {
                return Err(Error::Notes(format!(
                    "renaming paragraph nodes is not supported: {}",
                    new_path.display()
                )));
            }
            NodeKind::Heading(_) => {
                return Err(Error::Notes(format!(
                    "renaming heading nodes is not supported: {}",
                    new_path.display()
                )));
            }
        }

        if let Some(r) = file_rename {
            renames.push(r);
        }

        // Collect edits for incoming links.
        for (linker_id, edge) in graph.incoming(src) {
            let link = match edge.link() {
                Some(l) => l,
                None => continue,
            };
            let linker_path = match graph.node(linker_id) {
                NodeKind::Note(n) => n.path.clone(),
                NodeKind::Ghost(_) => continue,
                NodeKind::Directory(d) => d.path.clone(),
                NodeKind::Task(_) => continue,
                NodeKind::Paragraph(_) => continue,
                NodeKind::Heading(_) => continue,
            };
            // If the linker is also being renamed, use its new path
            // for relative-URL computation.
            let effective_linker_path = path_map.get(&linker_path).unwrap_or(&linker_path);

            let replacement = build_replacement(edge, effective_linker_path, &new_path, &new_title);
            edits.push(FileEdit {
                path: linker_path.clone(),
                byte_range: link.byte_range.clone(),
                replacement,
            });
            touched_files.insert(linker_path);
        }

        // Snapshot the source file.
        if let Some(ref src_rel) = source_rel_for_snapshot {
            touched_files.insert(src_rel.clone());
        }
    }

    // Build unique snapshots from all touched files.
    let mut snapshots: Vec<FileSnapshot> = Vec::new();
    for path in touched_files {
        snapshots.push(file_snapshot(vault_root, &path)?);
    }

    Ok(RenamePlan {
        renames,
        edits,
        snapshots,
    })
}

/// Apply a previously built [`RenamePlan`].
///
/// Order:
/// 1. Re-stat every snapshot; bail with `Error::Notes` if any
///    `(mtime, len)` differs from the snapshot.
/// 2. Validate non-overlap of edits within each file.
/// 3. Apply edits per file in **descending** byte order via
///    [`fs::write_atomic`].
/// 4. `std::fs::rename(from, to)` for every rename (creating `to`'s
///    parent dirs as needed).
/// 5. Best-effort empty-directory cleanup under old parent dirs
///    (deepest first; non-empty dirs are silently preserved).
///
/// Per-file atomicity is guaranteed; multi-file atomicity is not.
/// Documented under "Cross-file atomicity" in the module docs.
pub fn apply_rename_plan(vault_root: &Path, plan: &RenamePlan) -> Result<()> {
    // 1. Freshness check.
    for snap in &plan.snapshots {
        let abs = vault_root.join(&snap.path);
        let meta = std::fs::metadata(&abs).map_err(|e| Error::Io {
            path: abs.clone(),
            source: e,
        })?;
        let mtime = meta.modified().map_err(|e| Error::Io {
            path: abs.clone(),
            source: e,
        })?;
        if meta.len() != snap.len || mtime != snap.mtime {
            return Err(Error::Notes(format!(
                "file changed since plan was made: {} — re-plan and try again",
                snap.path.display()
            )));
        }
    }

    // 2 + 3. Group edits by path, validate non-overlap, apply
    // descending.
    let mut by_file: HashMap<PathBuf, Vec<&FileEdit>> = HashMap::new();
    for edit in &plan.edits {
        by_file.entry(edit.path.clone()).or_default().push(edit);
    }
    for (path, mut group) in by_file {
        group.sort_by_key(|e| std::cmp::Reverse(e.byte_range.start));
        // Non-overlap: in descending order, each edit's `start` must be
        // ≥ the next-already-processed edit's `end`. Equivalent in
        // ascending: each `end` ≤ next `start`.
        let mut ascending: Vec<&FileEdit> = group.iter().rev().copied().collect();
        ascending.sort_by_key(|e| e.byte_range.start);
        for pair in ascending.windows(2) {
            if pair[0].byte_range.end > pair[1].byte_range.start {
                return Err(Error::Notes(format!(
                    "planner produced overlapping edits for {} ({:?} vs {:?})",
                    path.display(),
                    pair[0].byte_range,
                    pair[1].byte_range,
                )));
            }
        }
        let abs = vault_root.join(&path);
        let mut content = std::fs::read_to_string(&abs).map_err(|e| Error::Io {
            path: abs.clone(),
            source: e,
        })?;
        for edit in &group {
            // `replace_range` panics on out-of-bounds or non-char
            // boundaries — the planner builds ranges from the parser's
            // byte_ranges, which are always char-aligned and in-bounds
            // at plan time. Freshness check ensures they still are.
            content.replace_range(edit.byte_range.clone(), &edit.replacement);
        }
        write_atomic(&abs, &content)?;
    }

    // 4. File renames — last so any in-self edits land at the old path
    // before the move. Apply in arbitrary order; parent dirs created as
    // needed.
    let mut old_dirs: std::collections::BTreeSet<PathBuf> = std::collections::BTreeSet::new();
    for rename in &plan.renames {
        let from = vault_root.join(&rename.from);
        let to = vault_root.join(&rename.to);
        if let Some(parent) = to.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent).map_err(|e| Error::Io {
                    path: parent.to_path_buf(),
                    source: e,
                })?;
            }
        }
        std::fs::rename(&from, &to).map_err(|e| Error::Io {
            path: from.clone(),
            source: e,
        })?;
        // Track old parent dirs for cleanup.
        if let Some(parent) = rename.from.parent() {
            if !parent.as_os_str().is_empty() {
                old_dirs.insert(parent.to_path_buf());
            }
        }
    }

    // 5. Best-effort empty directory cleanup.
    // Collect unique parent dirs from all renames, sort deepest-first
    // so we remove children before parents.
    if !old_dirs.is_empty() {
        let mut dirs: Vec<PathBuf> = old_dirs.into_iter().collect();
        dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
        for dir_rel in &dirs {
            let dir_abs = vault_root.join(dir_rel);
            // Only remove if it exists (it may already have been
            // removed as a descendant of a deeper rename) and is empty.
            let _ = std::fs::remove_dir(&dir_abs);
        }
    }

    Ok(())
}

/// Walk [`EdgeKind::Contains`] edges from `dir_id` via BFS to collect
/// all reachable notes with their new vault-relative paths. Used by
/// both the CLI `ft notes mv` and the TUI's directory rename / move
/// flows to expand a directory selection into individual note moves.
pub fn collect_directory_notes(
    graph: &Graph,
    dir_id: NoteId,
    old_dir: &Path,
    new_dir: &Path,
) -> Vec<(NoteId, PathBuf)> {
    let mut result: Vec<(NoteId, PathBuf)> = Vec::new();
    let mut queue: Vec<NoteId> = vec![dir_id];
    while let Some(current) = queue.pop() {
        for (child_id, edge) in graph.outgoing(current) {
            if !matches!(edge, EdgeKind::Contains) {
                continue;
            }
            match graph.node(child_id) {
                NodeKind::Note(n) => {
                    let old = n.path.clone();
                    let suffix = old.strip_prefix(old_dir).unwrap_or(&old);
                    let new = new_dir.join(suffix);
                    result.push((child_id, new));
                }
                NodeKind::Directory(_) => {
                    queue.push(child_id);
                }
                _ => {}
            }
        }
    }
    result
}

// ── helpers ──────────────────────────────────────────────────────────

fn file_snapshot(vault_root: &Path, rel: &Path) -> Result<FileSnapshot> {
    let abs = vault_root.join(rel);
    let meta = std::fs::metadata(&abs).map_err(|e| Error::Io {
        path: abs.clone(),
        source: e,
    })?;
    let mtime = meta.modified().map_err(|e| Error::Io {
        path: abs.clone(),
        source: e,
    })?;
    Ok(FileSnapshot {
        path: rel.to_path_buf(),
        mtime,
        len: meta.len(),
    })
}

/// Compute the replacement string for one incoming edge given the
/// new title/path of the renamed note. The replacement is the full
/// link token (including any leading `!` for embeds and any `[[…]]` /
/// `[…](…)` brackets).
fn build_replacement(
    edge: &EdgeKind,
    linker_path: &Path,
    new_path: &Path,
    new_title: &str,
) -> String {
    let link = edge
        .link()
        .expect("build_replacement called on Contains edge");
    let is_embed = matches!(edge, EdgeKind::Embed(_));
    match link.form {
        LinkForm::WikiLink => {
            let new_target = if link.target_text.contains('/') {
                // Path form: keep the path-form. Preserve a trailing
                // `.md` if the original had one, drop it if it didn't
                // (Obsidian accepts both).
                let kept_md = link.target_text.ends_with(".md");
                let stem_path = strip_md_ext(new_path);
                let mut s = stem_path.to_string_lossy().into_owned();
                if kept_md {
                    s.push_str(".md");
                }
                s
            } else {
                new_title.to_string()
            };
            let mut body = new_target;
            if let Some(anchor) = &link.anchor {
                body.push('#');
                body.push_str(anchor);
            }
            if let Some(display) = &link.display {
                body.push('|');
                body.push_str(display);
            }
            let prefix = if is_embed { "!" } else { "" };
            format!("{prefix}[[{body}]]")
        }
        LinkForm::MdLink => {
            let new_href = relative_url_from(linker_path, new_path);
            // Preserve the original "had explicit .md" decision.
            let original_had_md = link.target_text.ends_with(".md");
            let new_href = if original_had_md {
                new_href
            } else {
                strip_md_ext_str(&new_href)
            };
            let mut href = new_href;
            if let Some(anchor) = &link.anchor {
                href.push('#');
                href.push_str(anchor);
            }
            let display = link.display.clone().unwrap_or_default();
            let prefix = if is_embed { "!" } else { "" };
            format!("{prefix}[{display}]({href})")
        }
    }
}

fn strip_md_ext(p: &Path) -> PathBuf {
    if p.extension().is_some_and(|e| e == "md") {
        let stem = p.file_stem().unwrap_or_default();
        match p.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent.join(stem),
            _ => PathBuf::from(stem),
        }
    } else {
        p.to_path_buf()
    }
}

fn strip_md_ext_str(s: &str) -> String {
    if let Some(stripped) = s.strip_suffix(".md") {
        stripped.to_string()
    } else {
        s.to_string()
    }
}

/// Compute a `linker → target` path made relative to the linker's
/// directory, then URL-encode each path component (so spaces become
/// `%20` etc., matching what the parser decoded on the way in).
fn relative_url_from(linker_path: &Path, target_rel: &Path) -> String {
    use std::path::Component;
    let linker_dir: Vec<_> = linker_path
        .parent()
        .map(|p| p.components().collect())
        .unwrap_or_default();
    let target: Vec<Component> = target_rel.components().collect();

    // Find common prefix length.
    let common = linker_dir
        .iter()
        .zip(&target)
        .take_while(|(a, b)| a == b)
        .count();

    let ups = linker_dir.len() - common;
    let mut parts: Vec<String> = Vec::new();
    for _ in 0..ups {
        parts.push("..".to_string());
    }
    for c in &target[common..] {
        if let Component::Normal(s) = c {
            parts.push(urlencoding::encode(&s.to_string_lossy()).into_owned());
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod rename_tests {
    use super::*;
    use crate::graph::Graph;
    use crate::vault::{Scan, Vault};
    use assert_fs::prelude::*;
    use assert_fs::TempDir;
    use std::io::Write as _;

    /// Build a vault with the given files. Returns (TempDir keeping the
    /// directory alive, Vault, vault_root path).
    fn make_vault(files: &[(&str, &str)]) -> (TempDir, Vault, PathBuf) {
        let dir = TempDir::new().unwrap();
        dir.child(".obsidian").create_dir_all().unwrap();
        for (rel, content) in files {
            dir.child(rel).write_str(content).unwrap();
        }
        let vault = Vault::discover(Some(dir.path().to_path_buf())).unwrap();
        let root = vault.path.clone();
        (dir, vault, root)
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap()
    }

    fn note_id(graph: &Graph, rel: &str) -> NoteId {
        graph.note_by_path(Path::new(rel)).expect("note in graph")
    }

    // ── single linker, plain wikilink ────────────────────────────────

    #[test]
    fn rename_single_wikilink_linker_updates_file_and_link() {
        let (_dir, v, root) = make_vault(&[("foo.md", "# Foo\n"), ("a.md", "see [[foo]] now\n")]);
        let g = Graph::build(&v, &Scan::default()).unwrap();
        let foo = note_id(&g, "foo.md");
        let plan = plan_rename(&g, &root, foo, Path::new("bar.md")).unwrap();
        apply_rename_plan(&root, &plan).unwrap();

        assert!(!root.join("foo.md").exists());
        assert!(root.join("bar.md").exists());
        assert_eq!(read(&root.join("a.md")), "see [[bar]] now\n");
    }

    // ── multi-link in same file (descending byte order safety) ───────

    #[test]
    fn rename_multi_link_in_same_file_uses_descending_order_correctly() {
        // Three [[foo]] occurrences in one file → three edits in one
        // file. Every byte_range must remain valid after each replace.
        // Replacement is shorter than the original ([[foo]] → [[bar]]
        // is the same length here; force a length change by renaming
        // to a longer name to actually exercise the shift).
        let (_dir, v, root) = make_vault(&[
            ("foo.md", "# Foo\n"),
            ("a.md", "[[foo]] one [[foo]] two [[foo]] three\n"),
        ]);
        let g = Graph::build(&v, &Scan::default()).unwrap();
        let foo = note_id(&g, "foo.md");
        let plan = plan_rename(&g, &root, foo, Path::new("a-much-longer-name.md")).unwrap();
        apply_rename_plan(&root, &plan).unwrap();

        let after = read(&root.join("a.md"));
        let expected =
            "[[a-much-longer-name]] one [[a-much-longer-name]] two [[a-much-longer-name]] three\n";
        assert_eq!(after, expected);
    }

    // ── wikilink shape combinations ──────────────────────────────────

    #[test]
    fn rename_wikilink_with_display_preserves_alias() {
        let (_dir, v, root) = make_vault(&[
            ("foo.md", "# Foo\n"),
            ("a.md", "see [[foo|My Foo]] please\n"),
        ]);
        let g = Graph::build(&v, &Scan::default()).unwrap();
        let foo = note_id(&g, "foo.md");
        let plan = plan_rename(&g, &root, foo, Path::new("bar.md")).unwrap();
        apply_rename_plan(&root, &plan).unwrap();
        assert_eq!(read(&root.join("a.md")), "see [[bar|My Foo]] please\n");
    }

    #[test]
    fn rename_wikilink_with_anchor_preserves_anchor() {
        let (_dir, v, root) = make_vault(&[
            ("foo.md", "# Foo\n## H1\n"),
            ("a.md", "see [[foo#H1]] please\n"),
        ]);
        let g = Graph::build(&v, &Scan::default()).unwrap();
        let foo = note_id(&g, "foo.md");
        let plan = plan_rename(&g, &root, foo, Path::new("bar.md")).unwrap();
        apply_rename_plan(&root, &plan).unwrap();
        assert_eq!(read(&root.join("a.md")), "see [[bar#H1]] please\n");
    }

    #[test]
    fn rename_wikilink_with_anchor_and_display_preserves_both() {
        let (_dir, v, root) =
            make_vault(&[("foo.md", "# Foo\n## H1\n"), ("a.md", "see [[foo#H1|D]]\n")]);
        let g = Graph::build(&v, &Scan::default()).unwrap();
        let foo = note_id(&g, "foo.md");
        let plan = plan_rename(&g, &root, foo, Path::new("bar.md")).unwrap();
        apply_rename_plan(&root, &plan).unwrap();
        assert_eq!(read(&root.join("a.md")), "see [[bar#H1|D]]\n");
    }

    #[test]
    fn rename_wikilink_path_form_keeps_path_form() {
        // Linker uses `[[notes/foo]]`. Renaming `notes/foo.md` →
        // `notes/bar.md` should keep the path form.
        let (_dir, v, root) = make_vault(&[
            ("notes/foo.md", "# Foo\n"),
            ("a.md", "see [[notes/foo]] please\n"),
        ]);
        let g = Graph::build(&v, &Scan::default()).unwrap();
        let foo = note_id(&g, "notes/foo.md");
        let plan = plan_rename(&g, &root, foo, Path::new("notes/bar.md")).unwrap();
        apply_rename_plan(&root, &plan).unwrap();
        assert_eq!(read(&root.join("a.md")), "see [[notes/bar]] please\n");
    }

    #[test]
    fn rename_wikilink_path_form_with_md_suffix_kept() {
        let (_dir, v, root) = make_vault(&[
            ("notes/foo.md", "# Foo\n"),
            ("a.md", "see [[notes/foo.md]] please\n"),
        ]);
        let g = Graph::build(&v, &Scan::default()).unwrap();
        let foo = note_id(&g, "notes/foo.md");
        let plan = plan_rename(&g, &root, foo, Path::new("notes/bar.md")).unwrap();
        apply_rename_plan(&root, &plan).unwrap();
        assert_eq!(read(&root.join("a.md")), "see [[notes/bar.md]] please\n");
    }

    // ── markdown link ────────────────────────────────────────────────

    #[test]
    fn rename_md_link_updates_url_and_keeps_display() {
        let (_dir, v, root) = make_vault(&[
            ("foo.md", "# Foo\n"),
            ("a.md", "see [Click here](foo.md) please\n"),
        ]);
        let g = Graph::build(&v, &Scan::default()).unwrap();
        let foo = note_id(&g, "foo.md");
        let plan = plan_rename(&g, &root, foo, Path::new("bar.md")).unwrap();
        apply_rename_plan(&root, &plan).unwrap();
        assert_eq!(
            read(&root.join("a.md")),
            "see [Click here](bar.md) please\n"
        );
    }

    #[test]
    fn rename_md_link_extension_less_keeps_extension_less() {
        let (_dir, v, root) =
            make_vault(&[("foo.md", "# Foo\n"), ("a.md", "see [F](foo) please\n")]);
        let g = Graph::build(&v, &Scan::default()).unwrap();
        let foo = note_id(&g, "foo.md");
        let plan = plan_rename(&g, &root, foo, Path::new("bar.md")).unwrap();
        apply_rename_plan(&root, &plan).unwrap();
        assert_eq!(read(&root.join("a.md")), "see [F](bar) please\n");
    }

    #[test]
    fn rename_md_link_relative_to_linker_dir() {
        // Linker at notes/from.md links to ../foo.md; rename foo.md →
        // ../baz.md (i.e. baz.md). New href should still be `../baz.md`.
        let (_dir, v, root) = make_vault(&[
            ("foo.md", "# Foo\n"),
            ("notes/from.md", "see [F](../foo.md) please\n"),
        ]);
        let g = Graph::build(&v, &Scan::default()).unwrap();
        let foo = note_id(&g, "foo.md");
        let plan = plan_rename(&g, &root, foo, Path::new("baz.md")).unwrap();
        apply_rename_plan(&root, &plan).unwrap();
        assert_eq!(
            read(&root.join("notes/from.md")),
            "see [F](../baz.md) please\n"
        );
    }

    #[test]
    fn rename_md_link_with_anchor_preserves_anchor() {
        let (_dir, v, root) =
            make_vault(&[("foo.md", "# Foo\n## H\n"), ("a.md", "[F](foo.md#H)\n")]);
        let g = Graph::build(&v, &Scan::default()).unwrap();
        let foo = note_id(&g, "foo.md");
        let plan = plan_rename(&g, &root, foo, Path::new("bar.md")).unwrap();
        apply_rename_plan(&root, &plan).unwrap();
        assert_eq!(read(&root.join("a.md")), "[F](bar.md#H)\n");
    }

    // ── embeds ───────────────────────────────────────────────────────

    #[test]
    fn rename_wiki_embed_keeps_bang_prefix() {
        let (_dir, v, root) = make_vault(&[("foo.md", "# Foo\n"), ("a.md", "![[foo]]\n")]);
        let g = Graph::build(&v, &Scan::default()).unwrap();
        let foo = note_id(&g, "foo.md");
        let plan = plan_rename(&g, &root, foo, Path::new("bar.md")).unwrap();
        apply_rename_plan(&root, &plan).unwrap();
        assert_eq!(read(&root.join("a.md")), "![[bar]]\n");
    }

    #[test]
    fn rename_md_embed_keeps_bang_prefix() {
        let (_dir, v, root) = make_vault(&[("foo.md", "# Foo\n"), ("a.md", "![alt](foo.md)\n")]);
        let g = Graph::build(&v, &Scan::default()).unwrap();
        let foo = note_id(&g, "foo.md");
        let plan = plan_rename(&g, &root, foo, Path::new("bar.md")).unwrap();
        apply_rename_plan(&root, &plan).unwrap();
        assert_eq!(read(&root.join("a.md")), "![alt](bar.md)\n");
    }

    // ── self-link ────────────────────────────────────────────────────

    #[test]
    fn rename_self_link_edits_then_renames() {
        // foo.md contains a wikilink to itself: [[foo]]. After rename,
        // the file should be moved to bar.md AND its contents updated
        // to [[bar]].
        let (_dir, v, root) = make_vault(&[("foo.md", "see [[foo]] for self-reference\n")]);
        let g = Graph::build(&v, &Scan::default()).unwrap();
        let foo = note_id(&g, "foo.md");
        let plan = plan_rename(&g, &root, foo, Path::new("bar.md")).unwrap();
        apply_rename_plan(&root, &plan).unwrap();
        assert!(!root.join("foo.md").exists());
        assert_eq!(
            read(&root.join("bar.md")),
            "see [[bar]] for self-reference\n"
        );
    }

    // ── ghost rename ─────────────────────────────────────────────────

    #[test]
    fn rename_ghost_rewrites_linkers_without_creating_a_file() {
        // Two linkers point at [[Phantom]]. Rename ghost → Real.md.
        // Both linker files get rewritten; no file is created/renamed
        // (the user has to `ft notes create Real.md` separately).
        let (_dir, v, root) = make_vault(&[
            ("a.md", "see [[Phantom]]\n"),
            ("b.md", "also [[Phantom]]\n"),
        ]);
        let g = Graph::build(&v, &Scan::default()).unwrap();
        let phantom = g.ghost_by_raw("Phantom").unwrap();
        let plan = plan_rename(&g, &root, phantom, Path::new("Real.md")).unwrap();
        assert!(plan.renames.is_empty());
        apply_rename_plan(&root, &plan).unwrap();
        assert!(!root.join("Real.md").exists()); // not created
        assert_eq!(read(&root.join("a.md")), "see [[Real]]\n");
        assert_eq!(read(&root.join("b.md")), "also [[Real]]\n");
    }

    // ── error paths ──────────────────────────────────────────────────

    #[test]
    fn rename_to_existing_path_errors_before_any_writes() {
        let (_dir, v, root) = make_vault(&[
            ("foo.md", "# Foo\n"),
            ("bar.md", "# Bar (existing)\n"),
            ("a.md", "[[foo]]\n"),
        ]);
        let g = Graph::build(&v, &Scan::default()).unwrap();
        let foo = note_id(&g, "foo.md");
        let err = plan_rename(&g, &root, foo, Path::new("bar.md")).unwrap_err();
        assert!(format!("{err}").contains("target already exists"));
        // Neither foo.md nor bar.md were touched.
        assert!(root.join("foo.md").exists());
        assert_eq!(read(&root.join("bar.md")), "# Bar (existing)\n");
        assert_eq!(read(&root.join("a.md")), "[[foo]]\n");
    }

    #[test]
    fn rename_freshness_guard_trips_when_linker_changes_between_plan_and_apply() {
        let (_dir, v, root) = make_vault(&[("foo.md", "# Foo\n"), ("a.md", "[[foo]]\n")]);
        let g = Graph::build(&v, &Scan::default()).unwrap();
        let foo = note_id(&g, "foo.md");
        let plan = plan_rename(&g, &root, foo, Path::new("bar.md")).unwrap();

        // Mutate a.md out-of-band. Sleep 1.1s to guarantee mtime
        // resolution detects the change on filesystems with second-
        // grained mtimes (most macOS / linux defaults are nanosecond,
        // but this is defensive).
        std::thread::sleep(std::time::Duration::from_millis(20));
        let mut f = std::fs::File::create(root.join("a.md")).unwrap();
        writeln!(f, "totally different content").unwrap();
        drop(f);

        let err = apply_rename_plan(&root, &plan).unwrap_err();
        assert!(
            format!("{err}").contains("file changed since plan"),
            "got: {err}"
        );
        // The source file was *not* renamed because the applier bails
        // before reaching the rename step.
        assert!(root.join("foo.md").exists());
        assert!(!root.join("bar.md").exists());
    }

    // ── no-op + empty ────────────────────────────────────────────────

    #[test]
    fn rename_with_no_incoming_edges_just_renames_the_file() {
        let (_dir, v, root) = make_vault(&[("foo.md", "# Foo\n")]);
        let g = Graph::build(&v, &Scan::default()).unwrap();
        let foo = note_id(&g, "foo.md");
        let plan = plan_rename(&g, &root, foo, Path::new("bar.md")).unwrap();
        assert!(plan.edits.is_empty());
        apply_rename_plan(&root, &plan).unwrap();
        assert!(!root.join("foo.md").exists());
        assert!(root.join("bar.md").exists());
    }

    #[test]
    fn rename_to_same_path_is_a_noop() {
        let (_dir, v, root) = make_vault(&[("foo.md", "# Foo\n")]);
        let g = Graph::build(&v, &Scan::default()).unwrap();
        let foo = note_id(&g, "foo.md");
        let plan = plan_rename(&g, &root, foo, Path::new("foo.md")).unwrap();
        assert!(plan.renames.is_empty());
        assert!(plan.edits.is_empty());
        apply_rename_plan(&root, &plan).unwrap();
        assert!(root.join("foo.md").exists());
    }

    // ── url encoding for md links ────────────────────────────────────

    #[test]
    fn rename_md_link_to_path_with_spaces_url_encodes_in_href() {
        let (_dir, v, root) = make_vault(&[("foo.md", "# Foo\n"), ("a.md", "[F](foo.md)\n")]);
        let g = Graph::build(&v, &Scan::default()).unwrap();
        let foo = note_id(&g, "foo.md");
        let plan = plan_rename(&g, &root, foo, Path::new("My Note.md")).unwrap();
        apply_rename_plan(&root, &plan).unwrap();
        assert_eq!(read(&root.join("a.md")), "[F](My%20Note.md)\n");
    }

    // ── plan structure ───────────────────────────────────────────────

    #[test]
    fn touched_files_count_includes_source_and_linkers() {
        let (_dir, v, root) = make_vault(&[
            ("foo.md", "# Foo\n"),
            ("a.md", "[[foo]]\n"),
            ("b.md", "[[foo]]\n"),
        ]);
        let g = Graph::build(&v, &Scan::default()).unwrap();
        let foo = note_id(&g, "foo.md");
        let plan = plan_rename(&g, &root, foo, Path::new("bar.md")).unwrap();
        // foo.md (renamed) + a.md + b.md = 3
        assert_eq!(plan.touched_files(), 3);
    }

    // ── plan_multi_rename tests ──────────────────────────────────────

    #[test]
    fn multi_rename_single_note_matches_plan_rename() {
        let (_dir, v, root) = make_vault(&[("foo.md", "# Foo\n"), ("a.md", "[[foo]]\n")]);
        let g = Graph::build(&v, &Scan::default()).unwrap();
        let foo = note_id(&g, "foo.md");
        let plan1 = plan_rename(&g, &root, foo, Path::new("bar.md")).unwrap();
        let plan2 = plan_multi_rename(&g, &root, &[(foo, PathBuf::from("bar.md"))]).unwrap();
        assert_eq!(plan1.renames.len(), plan2.renames.len());
        assert_eq!(plan1.edits.len(), plan2.edits.len());
        assert_eq!(plan1.snapshots.len(), plan2.snapshots.len());
    }

    #[test]
    fn multi_rename_two_notes_no_cross_refs() {
        let (_dir, v, root) = make_vault(&[
            ("foo.md", "# Foo\n"),
            ("baz.md", "# Baz\n"),
            ("a.md", "[[foo]] and [[baz]]\n"),
        ]);
        let g = Graph::build(&v, &Scan::default()).unwrap();
        let foo = note_id(&g, "foo.md");
        let baz = note_id(&g, "baz.md");
        let plan = plan_multi_rename(
            &g,
            &root,
            &[
                (foo, PathBuf::from("bar.md")),
                (baz, PathBuf::from("qux.md")),
            ],
        )
        .unwrap();
        // Two renames + edits in a.md for both.
        assert_eq!(plan.renames.len(), 2);
        assert!(plan.edits.len() >= 2);
        apply_rename_plan(&root, &plan).unwrap();
        assert!(!root.join("foo.md").exists());
        assert!(!root.join("baz.md").exists());
        assert!(root.join("bar.md").exists());
        assert!(root.join("qux.md").exists());
        assert_eq!(read(&root.join("a.md")), "[[bar]] and [[qux]]\n");
    }

    #[test]
    fn multi_rename_with_cross_reference() {
        // foo.md links to baz.md, both renamed.
        let (_dir, v, root) = make_vault(&[("foo.md", "see [[baz]]\n"), ("baz.md", "# Baz\n")]);
        let g = Graph::build(&v, &Scan::default()).unwrap();
        let foo = note_id(&g, "foo.md");
        let baz = note_id(&g, "baz.md");
        let plan = plan_multi_rename(
            &g,
            &root,
            &[
                (foo, PathBuf::from("bar.md")),
                (baz, PathBuf::from("qux.md")),
            ],
        )
        .unwrap();
        apply_rename_plan(&root, &plan).unwrap();
        // foo.md → bar.md, baz.md → qux.md, foo's link updated.
        assert_eq!(read(&root.join("bar.md")), "see [[qux]]\n");
        assert_eq!(read(&root.join("qux.md")), "# Baz\n");
    }

    #[test]
    fn multi_rename_cross_reference_md_link_preserves_relative_path() {
        // x.md links to y.md via [note](y.md). Move both to sub/.
        // The relative link should stay (y.md) since both end up in the
        // same directory.
        let (_dir, v, root) = make_vault(&[("x.md", "see [note](y.md)\n"), ("y.md", "# Y\n")]);
        let g = Graph::build(&v, &Scan::default()).unwrap();
        let x = note_id(&g, "x.md");
        let y = note_id(&g, "y.md");
        let plan = plan_multi_rename(
            &g,
            &root,
            &[
                (x, PathBuf::from("sub/x.md")),
                (y, PathBuf::from("sub/y.md")),
            ],
        )
        .unwrap();
        apply_rename_plan(&root, &plan).unwrap();
        assert_eq!(read(&root.join("sub/x.md")), "see [note](y.md)\n");
        assert_eq!(read(&root.join("sub/y.md")), "# Y\n");
    }

    #[test]
    fn multi_rename_ghost_in_moves() {
        let (_dir, v, root) = make_vault(&[
            ("a.md", "see [[Phantom]]\n"),
            ("b.md", "also [[Phantom]]\n"),
        ]);
        let g = Graph::build(&v, &Scan::default()).unwrap();
        let phantom = g.ghost_by_raw("Phantom").unwrap();
        let plan = plan_multi_rename(&g, &root, &[(phantom, PathBuf::from("Real.md"))]).unwrap();
        assert!(plan.renames.is_empty());
        apply_rename_plan(&root, &plan).unwrap();
        assert_eq!(read(&root.join("a.md")), "see [[Real]]\n");
        assert_eq!(read(&root.join("b.md")), "also [[Real]]\n");
    }

    #[test]
    fn multi_rename_empty_moves_is_noop() {
        let (_dir, v, root) = make_vault(&[("foo.md", "# Foo\n")]);
        let g = Graph::build(&v, &Scan::default()).unwrap();
        let plan = plan_multi_rename(&g, &root, &[]).unwrap();
        assert!(plan.renames.is_empty());
        assert!(plan.edits.is_empty());
        apply_rename_plan(&root, &plan).unwrap();
        assert!(root.join("foo.md").exists());
    }

    #[test]
    fn multi_rename_same_path_is_skipped() {
        let (_dir, v, root) = make_vault(&[("foo.md", "# Foo\n"), ("a.md", "[[foo]]\n")]);
        let g = Graph::build(&v, &Scan::default()).unwrap();
        let foo = note_id(&g, "foo.md");
        let plan = plan_multi_rename(&g, &root, &[(foo, PathBuf::from("foo.md"))]).unwrap();
        assert!(plan.renames.is_empty());
        assert!(plan.edits.is_empty());
    }

    #[test]
    fn multi_rename_target_exists_errors() {
        let (_dir, v, root) =
            make_vault(&[("foo.md", "# Foo\n"), ("bar.md", "# Bar (existing)\n")]);
        let g = Graph::build(&v, &Scan::default()).unwrap();
        let foo = note_id(&g, "foo.md");
        let err = plan_multi_rename(&g, &root, &[(foo, PathBuf::from("bar.md"))]).unwrap_err();
        assert!(format!("{err}").contains("target already exists"));
    }

    #[test]
    fn multi_rename_three_files_directory_cleanup() {
        let (_dir, v, root) = make_vault(&[
            ("subdir/a.md", "# A\n"),
            ("subdir/b.md", "# B\n"),
            ("subdir/c.md", "# C\n"),
        ]);
        let g = Graph::build(&v, &Scan::default()).unwrap();
        let a = note_id(&g, "subdir/a.md");
        let b = note_id(&g, "subdir/b.md");
        let c = note_id(&g, "subdir/c.md");
        let plan = plan_multi_rename(
            &g,
            &root,
            &[
                (a, PathBuf::from("out/a.md")),
                (b, PathBuf::from("out/b.md")),
                (c, PathBuf::from("out/c.md")),
            ],
        )
        .unwrap();
        assert_eq!(plan.renames.len(), 3);
        apply_rename_plan(&root, &plan).unwrap();
        assert!(root.join("out/a.md").exists());
        assert!(root.join("out/b.md").exists());
        assert!(root.join("out/c.md").exists());
        // Old empty subdir should be removed.
        assert!(!root.join("subdir").exists());
    }

    #[test]
    fn multi_rename_non_empty_dir_preserved() {
        let (_dir, v, root) = make_vault(&[
            ("subdir/a.md", "# A\n"),
            ("subdir/b.md", "# B\n"),
            ("subdir/logo.png", "fake image\n"),
        ]);
        let g = Graph::build(&v, &Scan::default()).unwrap();
        let a = note_id(&g, "subdir/a.md");
        let plan = plan_multi_rename(&g, &root, &[(a, PathBuf::from("out/a.md"))]).unwrap();
        apply_rename_plan(&root, &plan).unwrap();
        assert!(root.join("out/a.md").exists());
        // subdir still exists (contains b.md + logo.png).
        assert!(root.join("subdir").exists());
        assert!(root.join("subdir/logo.png").exists());
    }

    #[test]
    fn multi_rename_freshness_check_across_files() {
        let (_dir, v, root) = make_vault(&[
            ("foo.md", "# Foo\n"),
            ("baz.md", "# Baz\n"),
            ("a.md", "[[foo]] [[baz]]\n"),
        ]);
        let g = Graph::build(&v, &Scan::default()).unwrap();
        let foo = note_id(&g, "foo.md");
        let baz = note_id(&g, "baz.md");
        let plan = plan_multi_rename(
            &g,
            &root,
            &[
                (foo, PathBuf::from("bar.md")),
                (baz, PathBuf::from("qux.md")),
            ],
        )
        .unwrap();
        // Mutate a.md out-of-band.
        std::thread::sleep(std::time::Duration::from_millis(20));
        let mut f = std::fs::File::create(root.join("a.md")).unwrap();
        writeln!(f, "different content").unwrap();
        drop(f);
        let err = apply_rename_plan(&root, &plan).unwrap_err();
        assert!(format!("{err}").contains("file changed since plan"));
        // No files were renamed.
        assert!(root.join("foo.md").exists());
        assert!(root.join("baz.md").exists());
    }
}
