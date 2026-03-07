//! Global paths for the walrus runtime.
//!
//! All crates resolve configuration, socket, and data paths through these
//! constants so there is a single source of truth.

use std::path::PathBuf;
use std::sync::LazyLock;

/// Global configuration directory (`~/.openwalrus/`).
pub static CONFIG_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    dirs::home_dir()
        .expect("no home directory")
        .join(".openwalrus")
});

/// Pinned socket path (`~/.openwalrus/walrus.sock`).
pub static SOCKET_PATH: LazyLock<PathBuf> = LazyLock::new(|| CONFIG_DIR.join("walrus.sock"));

/// Agents subdirectory (contains *.md files).
pub const AGENTS_DIR: &str = "agents";
/// Skills subdirectory.
pub const SKILLS_DIR: &str = "skills";
/// Data subdirectory.
pub const DATA_DIR: &str = "data";
/// Workspace sandbox subdirectory.
pub const WORK_DIR: &str = "work";

/// SQLite memory database filename.
pub const MEMORY_DB: &str = "memory.db";
