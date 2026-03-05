//! Memory backends for Walrus agents.
//!
//! Concrete implementations of the [`wcore::Memory`] trait:
//! [`InMemory`] (volatile) and [`SqliteMemory`] (persistent with FTS5 + vector recall).
//!
//! Memory abstractions (`Memory`, `Embedder`, `MemoryEntry`, `RecallOptions`) live in `wcore`.
//!
//! All SQL lives in `sql/*.sql` files, loaded via `include_str!`.

pub use mem::InMemory;
pub use sqlite::SqliteMemory;
use std::sync::Arc;
pub use wcore::{Embedder, Memory, MemoryEntry, RecallOptions, memory::tools};

mod mem;
mod sqlite;

/// Apply memory to an agent config — appends compiled memory to the system prompt.
pub fn with_memory(mut config: wcore::AgentConfig, memory: &impl Memory) -> wcore::AgentConfig {
    let compiled = memory.compile();
    if !compiled.is_empty() {
        config.system_prompt = format!("{}\n\n{compiled}", config.system_prompt);
    }
    config
}

impl wcore::Hook for InMemory {
    fn on_build_agent(&self, config: wcore::AgentConfig) -> wcore::AgentConfig {
        with_memory(config, self)
    }

    fn on_register_tools(
        &self,
        registry: &mut wcore::ToolRegistry,
    ) -> impl std::future::Future<Output = ()> + Send {
        let mem = Arc::new(self.clone());
        let (tool, handler) = tools::remember(Arc::clone(&mem));
        registry.insert(tool, handler);
        let (tool, handler) = tools::recall(mem);
        registry.insert(tool, handler);
        async {}
    }
}
