//! Note/directory deletion — pure planner + applier.
//!
//! [`plan_delete`] validates a vault-relative path is under the vault
//! root and produces a [`DeletePlan`] listing the paths to remove.
//! [`apply_delete`] removes files (via `fs::delete_file`) or directories
//! (via `fs::delete_directory`), then cleans up any empty parent
//! directories bottom-up.
//!
//! ## Best-effort empty-directory cleanup
//!
//! After removing the target, `apply_delete` walks every ancestor
//! directory of the deleted path and attempts `std::fs::remove_dir`.
//! Non-empty directories are silently preserved; only truly-empty
//! ancestors are cleaned up. This matches the [`crate::graph::rename`]
//! convention.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::fs;

/// A validated plan for deleting one file or directory tree.
#[derive(Debug, Clone)]
pub struct DeletePlan {
    /// Vault-absolute paths to remove. For a single file/directory
    /// this contains exactly one entry.
    pub paths: Vec<PathBuf>,
}

/// Validate a vault-relative `path` against `vault_root` and produce a
/// [`DeletePlan`].
///
/// Returns an error if `path` escapes the vault root (e.g. contains
/// `..` components that resolve above the root).
pub fn plan_delete(path: &Path, vault_root: &Path) -> Result<DeletePlan> {
    // Canonicalize vault_root so the symlink-resolved form matches what
    // `abs.canonicalize()` returns below. On macOS, $TMPDIR lives under
    // `/var`, which is a symlink to `/private/var`, so the un-resolved and
    // resolved forms would otherwise differ and the starts_with check
    // would spuriously reject every path.
    let vault_root = vault_root
        .canonicalize()
        .unwrap_or_else(|_| vault_root.to_path_buf());
    let abs = vault_root.join(path);

    // We do NOT require the path to exist yet — the applier will handle
    // NotFound gracefully. But we DO require the resolved path to be
    // under the vault root.
    let canonical = abs.canonicalize().unwrap_or_else(|_| {
        // Path doesn't exist yet — normalize manually to detect traversal.
        normalize_abs(&abs)
    });

    if !canonical.starts_with(&vault_root) {
        return Err(Error::Notes(format!(
            "path escapes vault root: {}",
            path.display()
        )));
    }

    Ok(DeletePlan { paths: vec![abs] })
}

/// Normalize an absolute path by collapsing `..` and `.` components
/// without touching the filesystem. Used when the path doesn't exist
/// yet so `canonicalize` would fail.
fn normalize_abs(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            c => out.push(c),
        }
    }
    out
}

/// Apply a delete plan: remove the target file/directory from disk,
/// then clean up empty parent directories bottom-up.
pub fn apply_delete(vault_root: &Path, plan: &DeletePlan) -> Result<()> {
    // Match the canonicalization performed in plan_delete so strip_prefix
    // succeeds when the original vault_root was symlinked (e.g. macOS
    // TMPDIR under /var → /private/var).
    let vault_root = vault_root
        .canonicalize()
        .unwrap_or_else(|_| vault_root.to_path_buf());
    // Collect parent dirs for cleanup before we delete the target.
    let mut parent_dirs: Vec<PathBuf> = Vec::new();
    for abs_path in &plan.paths {
        // Determine if this is a file or directory.
        let is_dir = abs_path
            .metadata()
            .ok()
            .map(|m| m.is_dir())
            .unwrap_or(false);

        // Track ancestors for cleanup.
        let rel = abs_path.strip_prefix(&vault_root).unwrap_or(abs_path);
        let mut ancestor = rel.parent().map(|p| p.to_path_buf());
        while let Some(a) = ancestor {
            if a.as_os_str().is_empty() {
                break;
            }
            parent_dirs.push(a.clone());
            ancestor = a.parent().map(|p| p.to_path_buf());
        }

        if is_dir {
            fs::delete_directory(abs_path)?;
        } else {
            fs::delete_file(abs_path)?;
        }
    }

    // Clean up empty parent directories bottom-up (deepest first).
    if !parent_dirs.is_empty() {
        parent_dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
        // Deduplicate.
        let mut seen: std::collections::BTreeSet<PathBuf> = std::collections::BTreeSet::new();
        for dir_rel in &parent_dirs {
            if !seen.insert(dir_rel.clone()) {
                continue;
            }
            let dir_abs = vault_root.join(dir_rel);
            let _ = std::fs::remove_dir(&dir_abs);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_fs::prelude::*;
    use assert_fs::TempDir;

    #[test]
    fn plan_delete_valid_path() {
        let vault = TempDir::new().unwrap();
        vault.child("notes/foo.md").write_str("hi").unwrap();

        let plan = plan_delete(Path::new("notes/foo.md"), vault.path()).unwrap();
        assert_eq!(plan.paths.len(), 1);
        // plan_delete canonicalizes vault_root, so compare against the
        // canonical form rather than the raw TempDir path (on macOS these
        // differ via the /var → /private/var symlink).
        assert!(plan.paths[0].starts_with(vault.path().canonicalize().unwrap()));
    }

    #[test]
    fn plan_delete_blocks_traversal() {
        let vault = TempDir::new().unwrap();
        // The path ../outside resolves above vault root.
        let res = plan_delete(Path::new("../outside"), vault.path());
        assert!(res.is_err());
    }

    #[test]
    fn apply_delete_removes_file() {
        let vault = TempDir::new().unwrap();
        vault.child("notes/foo.md").write_str("hi").unwrap();

        let plan = plan_delete(Path::new("notes/foo.md"), vault.path()).unwrap();
        apply_delete(vault.path(), &plan).unwrap();

        assert!(!vault.path().join("notes/foo.md").exists());
    }

    #[test]
    fn apply_delete_removes_directory() {
        let vault = TempDir::new().unwrap();
        vault.child("archive/a.md").write_str("a").unwrap();
        vault.child("archive/sub/b.md").write_str("b").unwrap();

        let plan = plan_delete(Path::new("archive"), vault.path()).unwrap();
        apply_delete(vault.path(), &plan).unwrap();

        assert!(!vault.path().join("archive").exists());
    }

    #[test]
    fn apply_delete_cleans_empty_parent() {
        let vault = TempDir::new().unwrap();
        vault.child("deep/nested/file.md").write_str("x").unwrap();

        let plan = plan_delete(Path::new("deep/nested/file.md"), vault.path()).unwrap();
        apply_delete(vault.path(), &plan).unwrap();

        assert!(!vault.path().join("deep/nested/file.md").exists());
        // nested/ was empty after file.md was deleted; should be removed.
        assert!(!vault.path().join("deep/nested").exists());
        // deep/ was empty after nested/ was removed; should be removed.
        assert!(!vault.path().join("deep").exists());
    }

    #[test]
    fn apply_delete_preserves_nonempty_parent() {
        let vault = TempDir::new().unwrap();
        vault.child("dir/a.md").write_str("a").unwrap();
        vault.child("dir/b.md").write_str("b").unwrap();

        let plan = plan_delete(Path::new("dir/a.md"), vault.path()).unwrap();
        apply_delete(vault.path(), &plan).unwrap();

        assert!(!vault.path().join("dir/a.md").exists());
        // dir/ still has b.md, should remain.
        assert!(vault.path().join("dir").exists());
        assert!(vault.path().join("dir/b.md").exists());
    }

    #[test]
    fn delete_file_nonexistent_succeeds() {
        let dir = TempDir::new().unwrap();
        fs::delete_file(&dir.path().join("nope.md")).unwrap();
    }

    #[test]
    fn delete_directory_nonexistent_succeeds() {
        let dir = TempDir::new().unwrap();
        fs::delete_directory(&dir.path().join("nope/")).unwrap();
    }
}
