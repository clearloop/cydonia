//! Fluent builder for constructing an [`Agent`].

use crate::{
    agent::{Agent, config::AgentConfig},
    model::Model,
};

/// Fluent builder for [`Agent<M>`].
///
/// Requires a model at construction. Use [`AgentConfig`] builder methods
/// for field configuration, then pass it via [`AgentBuilder::config`].
pub struct AgentBuilder<M: Model> {
    config: AgentConfig,
    model: M,
}

impl<M: Model> AgentBuilder<M> {
    /// Create a new builder with the given model.
    pub fn new(model: M) -> Self {
        Self {
            config: AgentConfig::default(),
            model,
        }
    }

    /// Set the full config, replacing all fields.
    ///
    /// Typical usage: build an `AgentConfig` via its fluent methods,
    /// then pass it here before calling `build()`.
    pub fn config(mut self, config: AgentConfig) -> Self {
        self.config = config;
        self
    }

    /// Build the [`Agent`].
    pub fn build(self) -> Agent<M> {
        Agent {
            config: self.config,
            model: self.model,
            history: Vec::new(),
        }
    }
}
