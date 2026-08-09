//! The single read pass over a vault: one walk (markdown files +
//! directories from the same walker pass) and one read per file,
//! extracting everything the model layer derives from files — task
//! lines, raw link occurrences, paragraph ranges, heading ranges, and
//! the raw `ft:` frontmatter block — into a [`Scan`].
//!
//! `Scan` is an immutable in-memory snapshot: [`crate::graph::Graph::build`]
//! constructs the graph entirely from it (directory nodes included) and
//! does no I/O of its own. [`crate::vault::Vault::scan`] is the
//! discovery-side convenience that produces it from the vault root and
//! the configured `ignored_paths`.

use std::path::{Path, PathBuf};

use ignore::{overrides::OverrideBuilder, WalkBuilder};
use rayon::prelude::*;
use tracing::debug;

use crate::{
    frontmatter,
    graph::parser::{extract_links, RawLink},
    markdown::{extract_headings, extract_paragraphs, Heading, LineSkipState, Paragraph},
    task::{
        emoji::EmojiFormat,
        format::{ParseContext, TaskFormat},
        hierarchy::resolve_hierarchy,
        Task,
    },
};

/// Folders excluded from scanning by default. Combined with `.gitignore` and
/// the vault's `ignored_paths` config.
///
/// Dotfile directories — `.obsidian/`, `.git/`, and `.ft/` — are additionally
/// excluded by the walker's `.hidden(true)` filter (see [`walk`]). `.ft/` is
/// intentionally absent from this list to avoid dead config: it would never
/// be the active exclusion path.
pub const DEFAULT_IGNORED: &[&str] = &[".obsidian", ".git", "attachments"];

/// Everything extracted from one markdown file in a single read during
/// [`scan_vault`]: the vault-relative path, the raw links, paragraph
/// ranges, and headings that [`crate::graph::Graph::build`] consumes,
/// plus the raw `ft:` frontmatter block for the block-taking readers in
/// [`crate::frontmatter`].
///
/// Struct (not a tuple) so the field reads stay self-documenting.
#[derive(Debug)]
pub struct ParsedFile {
    /// Vault-relative path.
    pub rel: PathBuf,
    pub links: Vec<RawLink>,
    pub paragraphs: Vec<Paragraph>,
    pub headings: Vec<Heading>,
    /// The raw frontmatter block text (between the YAML fences), when
    /// the file has one. Resolve keys via the `frontmatter::*_in`
    /// readers without re-reading the file.
    pub frontmatter: Option<String>,
}

/// Result of [`scan_vault`]. Tasks across the vault, the per-file parse
/// artifacts the graph build consumes, the directory list the graph
/// needs for directory nodes, plus per-file errors collected
/// non-fatally.
///
/// One scan is one walk plus one read of every vault file —
/// [`crate::graph::Graph::build`] works entirely from `files`/`dirs` and
/// does no I/O of its own, so `scan → build` touches each file exactly
/// once.
#[derive(Debug, Default)]
pub struct Scan {
    pub tasks: Vec<Task>,
    pub errors: Vec<ScanError>,
    /// Per-file parse artifacts, one entry per readable markdown file.
    pub files: Vec<ParsedFile>,
    /// Vault-relative directory paths from the same walk as `files`
    /// (root excluded; an empty path would be the root). Consistency
    /// with `files` is by construction: a directory excluded from the
    /// walk does not appear here, and neither do the files it contains.
    pub dirs: Vec<PathBuf>,
}

/// A non-fatal error encountered while scanning one file. Collected in
/// [`Scan::errors`] rather than aborting the whole scan.
#[derive(Debug)]
pub struct ScanError {
    /// Vault-relative path of the offending file (or absolute if it sits
    /// outside the vault root).
    pub path: PathBuf,
    pub message: String,
}

impl std::fmt::Display for ScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.message)
    }
}

/// Walk the vault and parse every markdown file in parallel, returning
/// all tasks, per-file parse artifacts, the directory list, plus
/// per-file errors. Respects `.gitignore`, hidden entries, default
/// exclusions (`.obsidian/`, `.git/`, `attachments/`), and the
/// `ignored_paths` config.
///
/// This is the single read pass: each file's content is loaded once and
/// everything downstream needs — tasks, links, paragraphs, headings,
/// frontmatter block — is extracted here.
/// [`crate::graph::Graph::build`] consumes the artifacts without
/// re-reading anything.
pub fn scan_vault(root: &Path, ignored: &[String]) -> Scan {
    let (files, dirs) = walk(root, ignored);
    debug!(file_count = files.len(), "starting parallel parse");

    let results: Vec<(Vec<Task>, Option<ParsedFile>, Option<ScanError>)> =
        files.par_iter().map(|rel| parse_file(root, rel)).collect();

    let mut scan = Scan::default();
    for (tasks, parsed, err) in results {
        scan.tasks.extend(tasks);
        if let Some(p) = parsed {
            scan.files.push(p);
        }
        if let Some(e) = err {
            scan.errors.push(e);
        }
    }
    scan.dirs = dirs;
    scan
}

/// Every markdown file under `root` as a vault-relative path, using the
/// same exclusion rules as [`scan_vault`]. Files whose metadata can't
/// be read are still returned (the metadata is only needed by
/// [`markdown_files_with_mtime`]).
pub fn markdown_files(root: &Path, ignored: &[String]) -> Vec<PathBuf> {
    walk(root, ignored).0
}

/// Walk the vault and pair each markdown file with its `mtime`.
///
/// Same exclusion rules as [`markdown_files`]. Files whose metadata
/// can't be read are kept in the result with mtime set to
/// `SystemTime::UNIX_EPOCH` so they still appear (last) in any recency
/// ranking rather than being silently dropped. Paths are vault-relative.
pub fn markdown_files_with_mtime(
    root: &Path,
    ignored: &[String],
) -> Vec<(PathBuf, std::time::SystemTime)> {
    walk(root, ignored)
        .0
        .into_iter()
        .map(|rel| {
            let mtime = std::fs::metadata(root.join(&rel))
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            (rel, mtime)
        })
        .collect()
}

/// One walker pass yielding both the markdown-file list and the
/// directory list, both vault-relative (root excluded from `dirs`).
///
/// The directory set is built from the same entries the file set is, so
/// the two can never disagree: a directory excluded from the walk does
/// not appear in `dirs`, and neither do the files it contains.
fn walk(root: &Path, ignored: &[String]) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut overrides = OverrideBuilder::new(root);
    for default in DEFAULT_IGNORED {
        // Exclude both the directory itself and its contents — the
        // walker's `is_dir()` check would otherwise let the dir entry
        // through even when its contents are filtered out.
        let _ = overrides.add(&format!("!{default}"));
        let _ = overrides.add(&format!("!{default}/**"));
    }
    for extra in ignored {
        let stripped = extra.strip_suffix('/').unwrap_or(extra);
        let _ = overrides.add(&format!("!{stripped}"));
        let _ = overrides.add(&format!("!{stripped}/**"));
    }
    let overrides = overrides.build().expect("override patterns are valid");

    let walker = WalkBuilder::new(root)
        .hidden(true)
        .ignore(true)
        .git_ignore(true)
        .git_exclude(true)
        .parents(false)
        .overrides(overrides)
        .build();

    let mut files = Vec::new();
    let mut dirs = Vec::new();
    for entry in walker.flatten() {
        let path = entry.path();
        let Some(rel) = path.strip_prefix(root).ok() else {
            continue;
        };
        let ft = entry.file_type();
        if ft.is_some_and(|t| t.is_dir()) {
            if !rel.as_os_str().is_empty() {
                dirs.push(rel.to_path_buf());
            }
        } else if ft.is_some_and(|t| t.is_file()) && path.extension().is_some_and(|e| e == "md") {
            files.push(rel.to_path_buf());
        }
    }
    (files, dirs)
}

/// Parse one markdown file into its task lines and graph parse
/// artifacts. The file is read exactly once here; everything the
/// downstream model needs is extracted from that single content.
fn parse_file(root: &Path, rel: &Path) -> (Vec<Task>, Option<ParsedFile>, Option<ScanError>) {
    let abs = root.join(rel);
    let content = match std::fs::read_to_string(&abs) {
        Ok(c) => c,
        Err(e) => {
            return (
                Vec::new(),
                None,
                Some(ScanError {
                    path: rel.to_path_buf(),
                    message: format!("read failed: {e}"),
                }),
            );
        }
    };

    // Task scan uses the same `LineSkipState` as `extract_paragraphs` /
    // `extract_headings`, so a `- [ ]` line inside a fenced code block or
    // YAML frontmatter is not recognized as a task. This keeps the invariant
    // that every task lands in exactly one paragraph (required for the
    // `OwnsTask` edge to be total) and avoids parsing code-block examples as
    // phantom tasks.
    let mut tasks = Vec::new();
    let mut skip_state = LineSkipState::new();
    for (lineno, line) in content.lines().enumerate() {
        if skip_state.skip_line(line) {
            continue;
        }
        let ctx = ParseContext {
            source_file: rel.to_path_buf(),
            source_line: lineno + 1,
        };
        if let Some(task) = EmojiFormat.parse_line(line, ctx) {
            tasks.push(task);
        }
    }
    resolve_hierarchy(&mut tasks);

    let parsed = ParsedFile {
        rel: rel.to_path_buf(),
        links: extract_links(&content),
        paragraphs: extract_paragraphs(&content),
        headings: extract_headings(&content),
        frontmatter: frontmatter::block(&content).map(str::to_string),
    };
    (tasks, Some(parsed), None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_fs::prelude::*;
    use assert_fs::TempDir;

    fn make_obsidian_dir(dir: &TempDir) {
        dir.child(".obsidian").create_dir_all().unwrap();
    }

    fn make_ft_dir(dir: &TempDir) {
        dir.child(".ft").create_dir_all().unwrap();
    }

    /// Build a temp vault via `Vault::discover` (so config loading and the
    /// `ignored_paths` seam are exercised end-to-end) and fill it with the
    /// given (rel, content) files.
    fn make_vault_with(files: &[(&str, &str)]) -> (TempDir, crate::vault::Vault) {
        let dir = TempDir::new().unwrap();
        make_obsidian_dir(&dir);
        for (rel, content) in files {
            let f = dir.child(rel);
            f.touch().unwrap();
            f.write_str(content).unwrap();
        }
        let vault = crate::vault::Vault::discover(Some(dir.path().to_path_buf())).unwrap();
        (dir, vault)
    }

    // ── scan ──────────────────────────────────────────────────────────────────

    #[test]
    fn scan_collects_tasks_from_multiple_files() {
        let (_dir, vault) = make_vault_with(&[
            ("a.md", "- [ ] task in A\n- [x] done in A ✅ 2026-05-01\n"),
            ("b.md", "Some prose\n- [ ] task in B\n"),
        ]);
        let scan = vault.scan();
        assert_eq!(scan.tasks.len(), 3, "expected 3 tasks total");
        assert!(scan.errors.is_empty());
    }

    #[test]
    fn scan_skips_default_excluded_dirs() {
        let (_dir, vault) = make_vault_with(&[
            ("notes/keep.md", "- [ ] keep me\n"),
            ("attachments/skip.md", "- [ ] skip me\n"),
        ]);
        let scan = vault.scan();
        let descs: Vec<_> = scan.tasks.iter().map(|t| t.description.clone()).collect();
        assert!(descs.contains(&"keep me".to_string()));
        assert!(!descs.contains(&"skip me".to_string()));
    }

    #[test]
    fn scan_respects_ignored_paths_from_config() {
        let dir = TempDir::new().unwrap();
        make_obsidian_dir(&dir);
        dir.child(".ft/config.toml")
            .write_str(r#"ignored_paths = ["private/"]"#)
            .unwrap();
        dir.child("public.md")
            .write_str("- [ ] public task\n")
            .unwrap();
        dir.child("private/secret.md")
            .write_str("- [ ] private task\n")
            .unwrap();

        let vault = crate::vault::Vault::discover(Some(dir.path().to_path_buf())).unwrap();
        let scan = vault.scan();
        let descs: Vec<_> = scan.tasks.iter().map(|t| t.description.clone()).collect();
        assert!(descs.contains(&"public task".to_string()));
        assert!(!descs.contains(&"private task".to_string()));
    }

    #[test]
    fn scan_excludes_ft_dir_contents() {
        // A `.ft/` directory is excluded by the walker's dotfile filter
        // (`.hidden(true)`), so tasks inside it are never scanned — even
        // when `.ft/` is the *only* vault marker present.
        let dir = TempDir::new().unwrap();
        make_ft_dir(&dir);
        dir.child(".ft/notes.md")
            .write_str("- [ ] hidden ft task\n")
            .unwrap();
        dir.child("visible.md")
            .write_str("- [ ] visible task\n")
            .unwrap();

        let vault = crate::vault::Vault::discover(Some(dir.path().to_path_buf())).unwrap();
        let scan = vault.scan();
        let descs: Vec<_> = scan.tasks.iter().map(|t| t.description.clone()).collect();
        assert!(descs.contains(&"visible task".to_string()));
        assert!(!descs.contains(&"hidden ft task".to_string()));
    }

    // ── task scanner skip rules ───────────────────────────────────────────────
    //
    // The task scan in `parse_file` uses `LineSkipState` so a `- [ ]` line
    // inside a fenced code block or YAML frontmatter is not parsed as a
    // task. This is the invariant the `OwnsTask` edge depends on (every
    // task lands in exactly one paragraph) and also a latent bug fix:
    // `- [ ]` lines in code blocks are examples, not tasks.

    #[test]
    fn scan_skips_task_lines_inside_fenced_code_blocks() {
        let (_dir, vault) = make_vault_with(&[(
            "note.md",
            "```
- [ ] example task in code block
```

- [ ] real task after code block
",
        )]);
        let scan = vault.scan();
        let descs: Vec<_> = scan.tasks.iter().map(|t| t.description.clone()).collect();
        assert!(
            descs.contains(&"real task after code block".to_string()),
            "real task after the code block must be parsed"
        );
        assert!(
            !descs.contains(&"example task in code block".to_string()),
            "task line inside a fenced code block must not be parsed"
        );
        assert_eq!(scan.tasks.len(), 1, "expected exactly one task");
    }

    #[test]
    fn scan_skips_task_lines_inside_frontmatter() {
        let (_dir, vault) = make_vault_with(&[(
            "note.md",
            "---
title: note
- [ ] fake task in frontmatter
---

- [ ] real task in body
",
        )]);
        let scan = vault.scan();
        let descs: Vec<_> = scan.tasks.iter().map(|t| t.description.clone()).collect();
        assert!(
            descs.contains(&"real task in body".to_string()),
            "real task in body must be parsed"
        );
        assert!(
            !descs.contains(&"fake task in frontmatter".to_string()),
            "task line inside frontmatter must not be parsed"
        );
        assert_eq!(scan.tasks.len(), 1, "expected exactly one task");
    }

    #[test]
    fn scan_parses_real_task_directly_after_code_block() {
        // The real task after the closing fence lands in its own paragraph
        // (single-line). Confirms the scanner re-engages after the fence
        // closes, not just that it skipped the fence.
        let (_dir, vault) = make_vault_with(&[(
            "note.md",
            "```
- [ ] inside
```
- [ ] immediately after
",
        )]);
        let scan = vault.scan();
        let descs: Vec<_> = scan.tasks.iter().map(|t| t.description.clone()).collect();
        assert!(descs.contains(&"immediately after".to_string()));
        assert!(!descs.contains(&"inside".to_string()));
        assert_eq!(scan.tasks.len(), 1);
    }

    #[test]
    fn scan_resolves_hierarchy_per_file() {
        let (_dir, vault) = make_vault_with(&[(
            "nested.md",
            "- [ ] parent\n  - [ ] child A\n  - [ ] child B\n",
        )]);
        let scan = vault.scan();
        assert_eq!(scan.tasks.len(), 3);
        let parent = scan
            .tasks
            .iter()
            .find(|t| t.description == "parent")
            .unwrap();
        let children: Vec<_> = scan
            .tasks
            .iter()
            .filter(|t| t.parent == Some(parent.source_line))
            .collect();
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn scan_returns_relative_paths() {
        let (_dir, vault) = make_vault_with(&[("notes/sub.md", "- [ ] task\n")]);
        let scan = vault.scan();
        assert_eq!(scan.tasks.len(), 1);
        assert_eq!(
            scan.tasks[0].source_file,
            std::path::PathBuf::from("notes/sub.md")
        );
    }

    // ── walker semantics ──────────────────────────────────────────────────────

    #[test]
    fn walk_yields_dirs_and_files_from_one_pass() {
        let dir = TempDir::new().unwrap();
        make_obsidian_dir(&dir);
        dir.child("a.md").write_str("x\n").unwrap();
        dir.child("sub/b.md").write_str("x\n").unwrap();

        let (files, dirs) = walk(dir.path(), &[]);
        assert_eq!(files.len(), 2, "both files found");
        assert_eq!(dirs, vec![std::path::PathBuf::from("sub")], "one dir");
        assert!(files.iter().all(|p| p.is_relative()));
        assert!(dirs.iter().all(|p| p.is_relative()));
    }

    #[test]
    fn walk_excludes_ignored_paths() {
        let dir = TempDir::new().unwrap();
        make_obsidian_dir(&dir);
        dir.child("keep.md").write_str("x\n").unwrap();
        dir.child("archive/drop.md").write_str("x\n").unwrap();
        dir.child("archive/keep-sub.md").write_str("x\n").unwrap();

        let (files, dirs) = walk(dir.path(), &["archive/".to_string()]);
        assert!(
            files.contains(&std::path::PathBuf::from("keep.md")),
            "unexcluded file present: {files:?}"
        );
        assert!(
            files.iter().all(|p| !p.starts_with("archive")),
            "archive files excluded: {files:?}"
        );
        assert!(
            dirs.iter()
                .all(|d| d != &std::path::PathBuf::from("archive")),
            "archive dir excluded: {dirs:?}"
        );
    }

    #[test]
    fn scan_captures_frontmatter_block() {
        let dir = TempDir::new().unwrap();
        make_obsidian_dir(&dir);
        dir.child("with.md")
            .write_str("---\nft:\n  synth:\n    enabled: true\n---\n\nbody\n")
            .unwrap();
        dir.child("without.md")
            .write_str("no frontmatter\n")
            .unwrap();

        let scan = scan_vault(dir.path(), &[]);
        let with = scan
            .files
            .iter()
            .find(|p| p.rel.ends_with("with.md"))
            .unwrap();
        assert_eq!(
            with.frontmatter.as_deref(),
            Some("ft:\n  synth:\n    enabled: true")
        );
        let without = scan
            .files
            .iter()
            .find(|p| p.rel.ends_with("without.md"))
            .unwrap();
        assert_eq!(without.frontmatter, None);
    }

    // ── scan → graph contract ────────────────────────────────────────────

    #[test]
    fn graph_builds_directory_nodes_from_scan_dirs() {
        // The dirs-fold guard: `Graph::build(scan)` derives Directory
        // nodes from `Scan::dirs` and Task nodes from `Scan::tasks`,
        // with no vault access.
        let dir = TempDir::new().unwrap();
        make_obsidian_dir(&dir);
        dir.child("notes/a.md")
            .write_str("- [ ] task in a\n[[b]]\n")
            .unwrap();
        dir.child("notes/sub/b.md")
            .write_str("- [ ] task in b\n")
            .unwrap();

        let scan = scan_vault(dir.path(), &[]);
        let graph = crate::graph::Graph::build(&scan).unwrap();

        // Every scanned directory became a Directory node.
        for d in &scan.dirs {
            assert!(
                graph.node_by_path(d).is_some(),
                "directory {d:?} missing from graph"
            );
        }
        // Every scanned task became a Task node at (path, line).
        for t in &scan.tasks {
            assert!(
                graph.task_by_loc(&t.source_file, t.source_line).is_some(),
                "task {} at {}:{} missing from graph",
                t.description,
                t.source_file.display(),
                t.source_line
            );
        }
        // The scan and graph agree on file counts.
        let note_count = graph
            .nodes()
            .filter(|(_, n)| matches!(n, crate::graph::NodeKind::Note(_)))
            .count();
        assert_eq!(note_count, scan.files.len());
    }
}
