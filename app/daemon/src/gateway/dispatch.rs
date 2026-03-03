//! Unified agent dispatch — the single point where all message sources converge.
//!
//! Both socket and channel messages flow through [`dispatch_send`] and
//! [`dispatch_stream`]. Per-agent locking via [`AgentLock`] prevents
//! concurrent take failures.

use crate::gateway::GatewayHook;
use compact_str::CompactString;
use futures_util::StreamExt;
use protocol::error::ProtocolError;
use protocol::message::StreamEvent;
use runtime::{AgentDispatcher, Runtime};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use wcore::AgentEvent;
use wcore::model::Message;

/// Per-agent execution lock.
///
/// Ensures only one message is processed at a time per agent. Other
/// messages queue up waiting for the lock. Different agents run concurrently.
pub struct AgentLock {
    locks: RwLock<BTreeMap<CompactString, Arc<Mutex<()>>>>,
}

impl Default for AgentLock {
    fn default() -> Self {
        Self {
            locks: RwLock::new(BTreeMap::new()),
        }
    }
}

impl AgentLock {
    /// Create a new empty lock set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Acquire the lock for the given agent name.
    async fn acquire(&self, agent: &str) -> tokio::sync::OwnedMutexGuard<()> {
        let lock = {
            let read = self.locks.read().await;
            if let Some(lock) = read.get(agent) {
                Arc::clone(lock)
            } else {
                drop(read);
                let mut write = self.locks.write().await;
                Arc::clone(write.entry(CompactString::from(agent)).or_default())
            }
        };
        lock.lock_owned().await
    }
}

/// Send a message to an agent and get the complete response.
pub async fn dispatch_send(
    runtime: &Runtime<GatewayHook>,
    locks: &AgentLock,
    agent: &str,
    content: &str,
) -> Result<String, ProtocolError> {
    let _guard = locks.acquire(agent).await;

    let Some(mut agent_instance) = runtime.take_agent(agent).await else {
        return Err(ProtocolError::new(
            404,
            format!("agent '{agent}' not registered"),
        ));
    };

    agent_instance.push_message(Message::user(content));
    let dispatcher = AgentDispatcher {
        hook: runtime.hook(),
        agent,
    };

    let response = agent_instance.run(&dispatcher).await;
    let content = response.final_response.unwrap_or_default();
    runtime.put_agent(agent_instance).await;

    Ok(content)
}

/// Send a message to an agent and stream response events.
pub fn dispatch_stream(
    runtime: Arc<Runtime<GatewayHook>>,
    locks: Arc<AgentLock>,
    agent: CompactString,
    content: String,
) -> impl futures_core::Stream<Item = Result<StreamEvent, ProtocolError>> + Send {
    async_stream::try_stream! {
        let _guard = locks.acquire(&agent).await;

        let Some(mut agent_instance) = runtime.take_agent(&agent).await else {
            Err(ProtocolError::new(404, format!("agent '{agent}' not registered")))?;
            return;
        };

        yield StreamEvent::Start { agent: agent.clone() };

        agent_instance.push_message(Message::user(&content));
        {
            let dispatcher = AgentDispatcher {
                hook: runtime.hook(),
                agent: &agent,
            };
            let stream = agent_instance.run_stream(&dispatcher);
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
        }

        yield StreamEvent::End { agent: agent.clone() };
        runtime.put_agent(agent_instance).await;
    }
}
