//! Memory backends for Walrus agents.
//!
//! Concrete implementations of the [`wcore::Memory`] trait:
//! [`InMemory`] (volatile), [`SqliteMemory`] (persistent with FTS5 + vector recall),
//! [`FsMemory`] (filesystem-backed TOML files, human-editable), and
//! [`LanceMemory`] (LanceDB-backed with hybrid vector + text search).
//!
//! Memory abstractions (`Memory`, `Embedder`, `MemoryEntry`, `RecallOptions`) live in `wcore`.
//!
//! All SQL lives in `sql/*.sql` files, loaded via `include_str!`.
//!
//! All `Memory` types that are `Clone + 'static` automatically implement `Hook`
//! via the blanket impl in `wcore::runtime::hook`.

pub use fs::FsMemory;
pub use lance::LanceMemory;
pub use mem::InMemory;
pub use sqlite::SqliteMemory;
pub use wcore::{Embedder, Memory, MemoryEntry, RecallOptions, memory::tools};

mod fs;
mod lance;
mod mem;
mod sqlite;
