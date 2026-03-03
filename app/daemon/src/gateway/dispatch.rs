//! Unified agent dispatch — the single point where all message sources converge.
//!
//! Both socket and channel messages flow through [`dispatch_send`] and
//! [`dispatch_stream`]. The runtime's take-execute-put pattern serializes
//! per-agent execution naturally.

use crate::gateway::GatewayHook;
use compact_str::CompactString;
use futures_util::StreamExt;
use model::ProviderManager;
use protocol::error::ProtocolError;
use protocol::message::StreamEvent;
use runtime::Runtime;
use std::sync::Arc;
use wcore::AgentEvent;

/// Send a message to an agent and get the complete response.
pub async fn dispatch_send(
    runtime: &Runtime<ProviderManager, GatewayHook>,
    agent: &str,
    content: &str,
) -> Result<String, ProtocolError> {
    runtime
        .send_to(agent, content)
        .await
        .map(|r| r.final_response.unwrap_or_default())
        .map_err(|e| ProtocolError::new(404, e.to_string()))
}

/// Send a message to an agent and stream response events.
pub fn dispatch_stream(
    runtime: Arc<Runtime<ProviderManager, GatewayHook>>,
    agent: CompactString,
    content: String,
) -> impl futures_core::Stream<Item = Result<StreamEvent, ProtocolError>> + Send {
    async_stream::try_stream! {
        yield StreamEvent::Start { agent: agent.clone() };

        let stream = runtime.stream_to(&agent, &content);
        futures_util::pin_mut!(stream);
        while let Some(event) = stream.next().await {
            match event {
                AgentEvent::TextDelta(text) => {
                    yield StreamEvent::Chunk { content: text };
                }
                AgentEvent::Done(_) => break,
                _ => {}
            }
        }

        yield StreamEvent::End { agent: agent.clone() };
    }
}
