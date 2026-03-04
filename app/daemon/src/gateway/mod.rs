//! Gateway — daemon core composing runtime and shared state.

use crate::hook::GatewayHook;
use model::ProviderManager;
use runtime::Runtime;
use std::sync::Arc;

pub(crate) mod builder;
pub(crate) mod event;
pub mod handle;
mod server;

/// Shared state available to all request handlers.
pub struct Gateway {
    /// The walrus runtime.
    pub runtime: Arc<Runtime<ProviderManager, GatewayHook>>,
    /// HuggingFace endpoint selected at startup (fastest of official/mirror).
    pub hf_endpoint: Arc<str>,
}

impl Clone for Gateway {
    fn clone(&self) -> Self {
        Self {
            runtime: Arc::clone(&self.runtime),
            hf_endpoint: Arc::clone(&self.hf_endpoint),
        }
    }
}
