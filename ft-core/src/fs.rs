use std::io::Write;
use std::path::Path;

use crate::error::{Error, Result};

/// Atomically write `content` to `path`.
///
/// Strategy: write to a temp file in the same directory as `path`, fsync,
/// then rename over `path`. Same-directory placement matters because rename
/// is only guaranteed atomic within a single filesystem on POSIX. Existing
/// file mode is preserved when the destination already exists.
pub fn write_atomic(path: &Path, content: &str) -> Result<()> {
    let dir = path.parent().ok_or_else(|| Error::Io {
        path: path.to_path_buf(),
        source: std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "path has no parent directory",
        ),
    })?;

    if !dir.as_os_str().is_empty() {
        std::fs::create_dir_all(dir).map_err(|e| Error::Io {
            path: dir.to_path_buf(),
            source: e,
        })?;
    }

    let mut tmp = tempfile::NamedTempFile::new_in(if dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        dir
    })
    .map_err(|e| Error::Io {
        path: path.to_path_buf(),
        source: e,
    })?;
    tmp.write_all(content.as_bytes()).map_err(|e| Error::Io {
        path: tmp.path().to_path_buf(),
        source: e,
    })?;
    tmp.as_file_mut().sync_all().map_err(|e| Error::Io {
        path: tmp.path().to_path_buf(),
        source: e,
    })?;

    #[cfg(unix)]
    if let Ok(meta) = std::fs::metadata(path) {
        use std::os::unix::fs::PermissionsExt;
        let mode = meta.permissions().mode();
        std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(mode)).map_err(
            |e| Error::Io {
                path: tmp.path().to_path_buf(),
                source: e,
            },
        )?;
    }

    tmp.persist(path).map_err(|e| Error::Io {
        path: path.to_path_buf(),
        source: e.error,
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_fs::prelude::*;
    use assert_fs::TempDir;

    #[test]
    fn writes_new_file() {
        let dir = TempDir::new().unwrap();
        let f = dir.child("out.md");
        write_atomic(f.path(), "hello\n").unwrap();
        assert_eq!(std::fs::read_to_string(f.path()).unwrap(), "hello\n");
    }

    #[test]
    fn overwrites_existing_file_atomically() {
        let dir = TempDir::new().unwrap();
        let f = dir.child("out.md");
        f.write_str("original\n").unwrap();
        write_atomic(f.path(), "replaced\n").unwrap();
        assert_eq!(std::fs::read_to_string(f.path()).unwrap(), "replaced\n");
    }

    #[test]
    fn creates_missing_parent_dirs() {
        let dir = TempDir::new().unwrap();
        let nested = dir.child("a/b/c.md");
        write_atomic(nested.path(), "inside\n").unwrap();
        assert_eq!(std::fs::read_to_string(nested.path()).unwrap(), "inside\n");
    }

    #[cfg(unix)]
    #[test]
    fn preserves_file_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new().unwrap();
        let f = dir.child("perm.md");
        f.write_str("seed\n").unwrap();
        std::fs::set_permissions(f.path(), std::fs::Permissions::from_mode(0o600)).unwrap();
        write_atomic(f.path(), "rewritten\n").unwrap();
        let mode = std::fs::metadata(f.path()).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    /// If a write is interrupted (we simulate by writing a tempfile and dropping it
    /// before persisting), the original file content must remain intact.
    #[test]
    fn original_unchanged_when_temp_dropped_before_persist() {
        let dir = TempDir::new().unwrap();
        let f = dir.child("safe.md");
        f.write_str("original\n").unwrap();

        {
            let tmp = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
            std::fs::write(tmp.path(), "partial").unwrap();
            // Drop without persisting → temp is removed; original untouched.
            drop(tmp);
        }

        assert_eq!(std::fs::read_to_string(f.path()).unwrap(), "original\n");
    }
}
