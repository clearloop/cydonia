//! Gateway mode — connect to walrusd via Unix domain socket.

use crate::runner::Runner;
use anyhow::Result;
use compact_str::CompactString;
use futures_core::Stream;
use futures_util::StreamExt;
use protocol::api::Client;
use protocol::message::{
    AgentDetail, AgentInfoRequest, AgentSummary, DownloadEvent, DownloadRequest, GetMemoryRequest,
    SendRequest, StreamEvent, StreamRequest,
};
use socket::{ClientConfig, Connection, WalrusClient};
use std::path::Path;

/// Runs agents via a walrusd Unix domain socket connection.
pub struct GatewayRunner {
    connection: Connection,
}

impl GatewayRunner {
    /// Connect to walrusd.
    pub async fn connect(socket_path: &Path) -> Result<Self> {
        let config = ClientConfig {
            socket_path: socket_path.to_path_buf(),
        };
        let client = WalrusClient::new(config);
        let connection = client.connect().await?;
        Ok(Self { connection })
    }

    /// List all registered agents.
    pub async fn list_agents(&mut self) -> Result<Vec<AgentSummary>> {
        let resp = self.connection.list_agents().await?;
        Ok(resp.agents)
    }

    /// Get detailed info for a specific agent.
    pub async fn agent_info(&mut self, agent: &str) -> Result<AgentDetail> {
        self.connection
            .agent_info(AgentInfoRequest {
                agent: CompactString::from(agent),
            })
            .await
    }

    /// List all memory entries.
    pub async fn list_memory(&mut self) -> Result<Vec<(String, String)>> {
        let resp = self.connection.list_memory().await?;
        Ok(resp.entries)
    }

    /// Send a download request and return a stream of progress events.
    pub fn download_stream(
        &mut self,
        model: &str,
    ) -> impl Stream<Item = Result<DownloadEvent>> + '_ {
        self.connection.download(DownloadRequest {
            model: CompactString::from(model),
        })
    }

    /// Get a specific memory entry by key.
    pub async fn get_memory(&mut self, key: &str) -> Result<Option<String>> {
        let resp = self
            .connection
            .get_memory(GetMemoryRequest {
                key: key.to_string(),
            })
            .await?;
        Ok(resp.value)
    }
}

impl Runner for GatewayRunner {
    async fn send(&mut self, agent: &str, content: &str) -> Result<String> {
        let resp = self
            .connection
            .send(SendRequest {
                agent: CompactString::from(agent),
                content: content.to_string(),
            })
            .await?;
        Ok(resp.content)
    }

    fn stream<'a>(
        &'a mut self,
        agent: &'a str,
        content: &'a str,
    ) -> impl Stream<Item = Result<String>> + Send + 'a {
        self.connection
            .stream(StreamRequest {
                agent: CompactString::from(agent),
                content: content.to_string(),
            })
            .filter_map(|result| async {
                match result {
                    Ok(StreamEvent::Chunk { content }) => Some(Ok(content)),
                    Ok(StreamEvent::Start { .. }) => None,
                    Ok(StreamEvent::End { .. }) => None,
                    Err(e) => Some(Err(e)),
                }
            })
    }
}
