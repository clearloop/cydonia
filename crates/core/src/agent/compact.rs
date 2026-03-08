//! Context compaction — summarize conversation history and replace it.

use crate::model::{Message, Model, Request};

const COMPACT_PROMPT: &str = include_str!("../../prompts/compact.md");

/// Sentinel prefix returned by the `compact` tool dispatch. When a tool result
/// starts with this prefix, the agent loop triggers compaction.
pub const COMPACT_SENTINEL: &str = "__COMPACT__";

impl<M: Model> super::Agent<M> {
    /// Summarize the current conversation history using the LLM.
    ///
    /// Sends the full history with the compact prompt as system message.
    /// Returns the summary text, or `None` if the model produces no content.
    pub(crate) async fn compact(&mut self) -> Option<String> {
        let model_name = self
            .config
            .model
            .clone()
            .unwrap_or_else(|| self.model.active_model());

        let mut messages = Vec::with_capacity(1 + self.history.len());
        messages.push(Message::system(COMPACT_PROMPT));
        messages.extend(self.history.iter().cloned());

        let request = Request::new(model_name).with_messages(messages);
        match self.model.send(&request).await {
            Ok(response) => response.content().cloned(),
            Err(e) => {
                tracing::warn!("compaction LLM call failed: {e}");
                None
            }
        }
    }
}
