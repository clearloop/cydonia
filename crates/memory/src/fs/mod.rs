//! Filesystem-based implementation of the Memory trait.
//!
//! Keys are mapped to directory paths using `.` as a separator:
//! `user.goal.abc` → `{base}/user/goal/abc.md`.
//! File content is raw Markdown — the value itself, no frontmatter.
//! Reads are always fresh from disk, so manual edits are immediately visible.

use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};
use wcore::Memory;

/// Filesystem memory store backed by Markdown files.
///
/// `Clone` is cheap — clones share the same base path.
#[derive(Debug, Clone)]
pub struct FsMemory {
    base: Arc<PathBuf>,
}

impl FsMemory {
    /// Create a new store rooted at `base`, creating the directory if needed.
    pub fn new(base: impl Into<PathBuf>) -> io::Result<Self> {
        let base = base.into();
        fs::create_dir_all(&base)?;
        Ok(Self {
            base: Arc::new(base),
        })
    }

    fn key_to_path(&self, key: &str) -> PathBuf {
        let mut path = (*self.base).clone();
        let parts: Vec<&str> = key.split('.').collect();
        for part in &parts {
            path.push(part);
        }
        path.set_extension("md");
        path
    }

    fn path_to_key(base: &Path, path: &Path) -> Option<String> {
        let rel = path.strip_prefix(base).ok()?;
        let without_ext = rel.with_extension("");
        let key = without_ext
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join(".");
        if key.is_empty() { None } else { Some(key) }
    }

    fn collect_entries(dir: &Path, base: &Path, out: &mut Vec<(String, String)>) {
        let Ok(read) = fs::read_dir(dir) else {
            return;
        };
        for entry in read.flatten() {
            let path = entry.path();
            if path.is_dir() {
                Self::collect_entries(&path, base, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("md")
                && let (Some(key), Ok(value)) =
                    (Self::path_to_key(base, &path), fs::read_to_string(&path))
            {
                out.push((key, value));
            }
        }
    }
}

impl Memory for FsMemory {
    fn get(&self, key: &str) -> Option<String> {
        let path = self.key_to_path(key);
        fs::read_to_string(path).ok()
    }

    fn entries(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        Self::collect_entries(&self.base, &self.base, &mut out);
        out
    }

    fn set(&self, key: impl Into<String>, value: impl Into<String>) -> Option<String> {
        let key = key.into();
        let value = value.into();
        let path = self.key_to_path(&key);

        // Capture old value for return.
        let old = fs::read_to_string(&path).ok();

        // Create parent directories.
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).ok()?;
        }

        // Atomic write: write to .tmp then rename.
        let tmp = path.with_extension("md.tmp");
        fs::write(&tmp, &value).ok()?;
        fs::rename(&tmp, &path).ok()?;

        old
    }

    fn remove(&self, key: &str) -> Option<String> {
        let path = self.key_to_path(key);
        let old = fs::read_to_string(&path).ok()?;
        fs::remove_file(&path).ok()?;

        // Remove empty parent directories up to base.
        let mut current = path.parent();
        while let Some(dir) = current {
            if dir == *self.base {
                break;
            }
            if fs::read_dir(dir)
                .map(|mut d| d.next().is_none())
                .unwrap_or(false)
            {
                fs::remove_dir(dir).ok();
            } else {
                break;
            }
            current = dir.parent();
        }

        Some(old)
    }
}
