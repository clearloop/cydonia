//! Client trait — transport primitives plus typed provided methods.

use crate::message::{
    AgentDetail, AgentInfoRequest, AgentList, ClearSessionRequest, DownloadEvent, DownloadRequest,
    GetMemoryRequest, McpAddRequest, McpAdded, McpReloaded, McpRemoveRequest, McpRemoved,
    McpServerList, MemoryEntry, MemoryList, SendRequest, SendResponse, SessionCleared,
    SkillsReloaded, StreamEvent, StreamRequest, client::ClientMessage, server::ServerMessage,
};
use anyhow::Result;
use futures_core::Stream;
use futures_util::StreamExt;

/// Client-side protocol interface.
///
/// Implementors provide two transport primitives — [`request`](Client::request)
/// for request-response and [`request_stream`](Client::request_stream) for
/// streaming operations. All typed methods are provided defaults that delegate
/// to these primitives.
pub trait Client: Send {
    /// Send a `ClientMessage` and receive a single `ServerMessage`.
    fn request(
        &mut self,
        msg: ClientMessage,
    ) -> impl std::future::Future<Output = Result<ServerMessage>> + Send;

    /// Send a `ClientMessage` and receive a stream of `ServerMessage`s.
    ///
    /// This is a raw transport primitive — the stream reads indefinitely.
    /// Callers must detect the terminal sentinel (e.g. `StreamEnd`,
    /// `DownloadEnd`) and stop consuming. The typed streaming methods
    /// handle this automatically.
    fn request_stream(
        &mut self,
        msg: ClientMessage,
    ) -> impl Stream<Item = Result<ServerMessage>> + Send + '_;

    /// Send a message to an agent and receive a complete response.
    fn send(
        &mut self,
        req: SendRequest,
    ) -> impl std::future::Future<Output = Result<SendResponse>> + Send {
        async move { SendResponse::try_from(self.request(req.into()).await?) }
    }

    /// Send a message to an agent and receive a streamed response.
    fn stream(
        &mut self,
        req: StreamRequest,
    ) -> impl Stream<Item = Result<StreamEvent>> + Send + '_ {
        self.request_stream(req.into())
            .scan(false, |done, r| {
                if *done {
                    return std::future::ready(None);
                }
                if matches!(&r, Ok(ServerMessage::StreamEnd { .. })) {
                    *done = true;
                }
                std::future::ready(Some(r))
            })
            .map(|r| r.and_then(StreamEvent::try_from))
    }

    /// Clear the session history for an agent.
    fn clear_session(
        &mut self,
        req: ClearSessionRequest,
    ) -> impl std::future::Future<Output = Result<SessionCleared>> + Send {
        async move { SessionCleared::try_from(self.request(req.into()).await?) }
    }

    /// List all registered agents.
    fn list_agents(&mut self) -> impl std::future::Future<Output = Result<AgentList>> + Send {
        async move { AgentList::try_from(self.request(ClientMessage::ListAgents).await?) }
    }

    /// Get detailed info for a specific agent.
    fn agent_info(
        &mut self,
        req: AgentInfoRequest,
    ) -> impl std::future::Future<Output = Result<AgentDetail>> + Send {
        async move { AgentDetail::try_from(self.request(req.into()).await?) }
    }

    /// List all memory entries.
    fn list_memory(&mut self) -> impl std::future::Future<Output = Result<MemoryList>> + Send {
        async move { MemoryList::try_from(self.request(ClientMessage::ListMemory).await?) }
    }

    /// Get a specific memory entry by key.
    fn get_memory(
        &mut self,
        req: GetMemoryRequest,
    ) -> impl std::future::Future<Output = Result<MemoryEntry>> + Send {
        async move { MemoryEntry::try_from(self.request(req.into()).await?) }
    }

    /// Download a model's files with progress reporting.
    fn download(
        &mut self,
        req: DownloadRequest,
    ) -> impl Stream<Item = Result<DownloadEvent>> + Send + '_ {
        self.request_stream(req.into())
            .scan(false, |done, r| {
                if *done {
                    return std::future::ready(None);
                }
                if matches!(&r, Ok(ServerMessage::DownloadEnd { .. })) {
                    *done = true;
                }
                std::future::ready(Some(r))
            })
            .map(|r| r.and_then(DownloadEvent::try_from))
    }

    /// Reload skills from disk.
    fn reload_skills(
        &mut self,
    ) -> impl std::future::Future<Output = Result<SkillsReloaded>> + Send {
        async move { SkillsReloaded::try_from(self.request(ClientMessage::ReloadSkills).await?) }
    }

    /// Add an MCP server.
    fn mcp_add(
        &mut self,
        req: McpAddRequest,
    ) -> impl std::future::Future<Output = Result<McpAdded>> + Send {
        async move { McpAdded::try_from(self.request(req.into()).await?) }
    }

    /// Remove an MCP server.
    fn mcp_remove(
        &mut self,
        req: McpRemoveRequest,
    ) -> impl std::future::Future<Output = Result<McpRemoved>> + Send {
        async move { McpRemoved::try_from(self.request(req.into()).await?) }
    }

    /// Reload MCP servers from config.
    fn mcp_reload(&mut self) -> impl std::future::Future<Output = Result<McpReloaded>> + Send {
        async move { McpReloaded::try_from(self.request(ClientMessage::McpReload).await?) }
    }

    /// List connected MCP servers.
    fn mcp_list(&mut self) -> impl std::future::Future<Output = Result<McpServerList>> + Send {
        async move { McpServerList::try_from(self.request(ClientMessage::McpList).await?) }
    }

    /// Ping the server (keepalive).
    fn ping(&mut self) -> impl std::future::Future<Output = Result<()>> + Send {
        async move {
            match self.request(ClientMessage::Ping).await? {
                ServerMessage::Pong => Ok(()),
                ServerMessage::Error { code, message } => {
                    anyhow::bail!("server error ({code}): {message}")
                }
                other => anyhow::bail!("unexpected response: {other:?}"),
            }
        }
    }
}
