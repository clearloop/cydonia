//! Filesystem-based implementation of the Memory trait using TOML files.
//!
//! The key scheme splits on `.`: the first segment is the TOML filename, and
//! the remaining segments form a nested TOML key path. For example:
//!
//! - `user.name`         → `{base}/user.toml`, key `name`
//! - `user.goal.coding`  → `{base}/user.toml`, nested `[goal]` → `coding`
//! - `soul.relationship` → `{base}/soul.toml`, key `relationship`
//!
//! Files are format-preserving via `toml_edit`. Reads are always fresh from
//! disk, so manual edits are immediately visible.

use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};
use toml_edit::{DocumentMut, Item, Table, value};
use wcore::Memory;

/// Filesystem memory store backed by per-namespace TOML files.
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

    /// Path to `{base}/{ns}.toml`.
    fn file_path(&self, ns: &str) -> PathBuf {
        self.base.join(format!("{ns}.toml"))
    }

    /// Split `key` into `(namespace, key_segments)`.
    ///
    /// Returns `None` if the key has fewer than 2 segments.
    fn split_key(key: &str) -> Option<(&str, Vec<&str>)> {
        let (ns, rest) = key.split_once('.')?;
        let segs: Vec<&str> = rest.split('.').collect();
        if ns.is_empty() || segs.iter().any(|s| s.is_empty()) {
            return None;
        }
        Some((ns, segs))
    }

    /// Load a TOML file, returning an empty document if the file does not exist.
    fn load(path: &Path) -> DocumentMut {
        fs::read_to_string(path)
            .ok()
            .and_then(|s| s.parse::<DocumentMut>().ok())
            .unwrap_or_default()
    }

    /// Atomically write a TOML document to `path` via a `.tmp` file + rename.
    fn save(path: &Path, doc: &DocumentMut) {
        let tmp = path.with_extension("toml.tmp");
        if fs::write(&tmp, doc.to_string()).is_ok() {
            let _ = fs::rename(&tmp, path);
        }
    }

    /// Walk `item` through `segs` and return the string value at the leaf.
    fn get_nested<'a>(item: &'a Item, segs: &[&str]) -> Option<&'a str> {
        if segs.is_empty() {
            return item.as_str();
        }
        Self::get_nested(&item[segs[0]], &segs[1..])
    }

    /// Recursively collect all leaf string values from a TOML table, building
    /// full key paths by prepending `prefix`.
    fn collect_leaves(table: &Table, prefix: &str, out: &mut Vec<(String, String)>) {
        for (k, v) in table {
            let full_key = if prefix.is_empty() {
                k.to_owned()
            } else {
                format!("{prefix}.{k}")
            };
            match v {
                Item::Value(toml_edit::Value::String(s)) => {
                    out.push((full_key, s.value().clone()));
                }
                Item::Table(t) => {
                    Self::collect_leaves(t, &full_key, out);
                }
                _ => {}
            }
        }
    }

    /// Ensure all intermediate tables along `segs[..segs.len()-1]` exist in `doc`,
    /// then insert `value` at the leaf `segs[last]`. Returns the previous string
    /// value at that path if one existed.
    fn insert_nested(doc: &mut DocumentMut, segs: &[&str], val: &str) -> Option<String> {
        // Walk/create intermediate tables.
        let mut table: *mut Table = doc.as_table_mut();
        for &seg in &segs[..segs.len() - 1] {
            // Safety: we hold &mut doc for the duration of this function.
            let t = unsafe { &mut *table };
            if !t.contains_key(seg) {
                t.insert(seg, Item::Table(Table::new()));
            }
            table = match unsafe { &mut *table }.get_mut(seg) {
                Some(Item::Table(t)) => t as *mut Table,
                _ => return None,
            };
        }
        let leaf = segs[segs.len() - 1];
        let t = unsafe { &mut *table };
        let old = t.get(leaf).and_then(|i| i.as_str()).map(ToOwned::to_owned);
        t.insert(leaf, value(val));
        old
    }

    /// Remove the leaf at `segs` from `doc`. Returns the previous value if any.
    /// Prunes empty parent tables bottom-up.
    fn remove_nested(doc: &mut DocumentMut, segs: &[&str]) -> Option<String> {
        if segs.len() == 1 {
            return doc
                .remove(segs[0])
                .and_then(|i| i.into_value().ok())
                .and_then(|v| {
                    if let toml_edit::Value::String(s) = v {
                        Some(s.into_value())
                    } else {
                        None
                    }
                });
        }
        // For nested paths, rebuild the path and prune after removal.
        // Walk to the parent table, remove leaf, then prune empty tables.
        Self::remove_nested_recursive(doc.as_table_mut(), segs)
    }

    fn remove_nested_recursive(table: &mut Table, segs: &[&str]) -> Option<String> {
        if segs.len() == 1 {
            return table
                .remove(segs[0])
                .and_then(|i| i.into_value().ok())
                .and_then(|v| {
                    if let toml_edit::Value::String(s) = v {
                        Some(s.into_value())
                    } else {
                        None
                    }
                });
        }
        let child = table.get_mut(segs[0])?;
        let child_table = child.as_table_mut()?;
        let old = Self::remove_nested_recursive(child_table, &segs[1..]);
        if old.is_some() && child_table.is_empty() {
            table.remove(segs[0]);
        }
        old
    }
}

impl Memory for FsMemory {
    fn get(&self, key: &str) -> Option<String> {
        let (ns, segs) = Self::split_key(key)?;
        let doc = Self::load(&self.file_path(ns));
        Self::get_nested(doc.as_item(), &segs).map(ToOwned::to_owned)
    }

    fn entries(&self) -> Vec<(String, String)> {
        let Ok(read_dir) = fs::read_dir(&*self.base) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let doc = Self::load(&path);
            Self::collect_leaves(doc.as_table(), stem, &mut out);
        }
        out
    }

    fn set(&self, key: impl Into<String>, value: impl Into<String>) -> Option<String> {
        let key = key.into();
        let value = value.into();
        let (ns, segs) = Self::split_key(&key)?;
        let path = self.file_path(ns);
        let mut doc = Self::load(&path);
        let old = Self::insert_nested(&mut doc, &segs, &value);
        Self::save(&path, &doc);
        old
    }

    fn remove(&self, key: &str) -> Option<String> {
        let (ns, segs) = Self::split_key(key)?;
        let path = self.file_path(ns);
        let mut doc = Self::load(&path);
        let old = Self::remove_nested(&mut doc, &segs);
        if old.is_some() {
            if doc.as_table().is_empty() {
                let _ = fs::remove_file(&path);
            } else {
                Self::save(&path, &doc);
            }
        }
        old
    }
}
