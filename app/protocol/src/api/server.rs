//! Server trait — one async method per protocol operation.

use crate::error::ProtocolError;
use crate::message::{
    AgentDetail, AgentInfoRequest, AgentList, ClearSessionRequest, DownloadEvent, DownloadRequest,
    GetMemoryRequest, McpAddRequest, McpAdded, McpReloaded, McpRemoveRequest, McpRemoved,
    McpServerList, MemoryEntry, MemoryList, SendRequest, SendResponse, SessionCleared,
    SkillsReloaded, StreamEvent, StreamRequest,
};
use futures_core::Stream;

/// Server-side protocol handler.
///
/// Each method corresponds to one `ClientMessage` variant. Implementations
/// receive typed request structs and return typed responses — no enum matching
/// required. Streaming operations return `impl Stream`.
pub trait Server {
    /// Handle `Send` — run agent and return complete response.
    fn send(
        &self,
        req: SendRequest,
    ) -> impl std::future::Future<Output = Result<SendResponse, ProtocolError>> + Send;

    /// Handle `Stream` — run agent and stream response events.
    fn stream(
        &self,
        req: StreamRequest,
    ) -> impl Stream<Item = Result<StreamEvent, ProtocolError>> + Send;

    /// Handle `ClearSession` — clear agent history.
    fn clear_session(
        &self,
        req: ClearSessionRequest,
    ) -> impl std::future::Future<Output = Result<SessionCleared, ProtocolError>> + Send;

    /// Handle `ListAgents` — list all registered agents.
    fn list_agents(
        &self,
    ) -> impl std::future::Future<Output = Result<AgentList, ProtocolError>> + Send;

    /// Handle `AgentInfo` — get agent details.
    fn agent_info(
        &self,
        req: AgentInfoRequest,
    ) -> impl std::future::Future<Output = Result<AgentDetail, ProtocolError>> + Send;

    /// Handle `ListMemory` — list all memory entries.
    fn list_memory(
        &self,
    ) -> impl std::future::Future<Output = Result<MemoryList, ProtocolError>> + Send;

    /// Handle `GetMemory` — get a memory entry by key.
    fn get_memory(
        &self,
        req: GetMemoryRequest,
    ) -> impl std::future::Future<Output = Result<MemoryEntry, ProtocolError>> + Send;

    /// Handle `Download` — download model files with progress.
    fn download(
        &self,
        req: DownloadRequest,
    ) -> impl Stream<Item = Result<DownloadEvent, ProtocolError>> + Send;

    /// Handle `ReloadSkills` — reload skills from disk.
    fn reload_skills(
        &self,
    ) -> impl std::future::Future<Output = Result<SkillsReloaded, ProtocolError>> + Send;

    /// Handle `McpAdd` — add an MCP server.
    fn mcp_add(
        &self,
        req: McpAddRequest,
    ) -> impl std::future::Future<Output = Result<McpAdded, ProtocolError>> + Send;

    /// Handle `McpRemove` — remove an MCP server.
    fn mcp_remove(
        &self,
        req: McpRemoveRequest,
    ) -> impl std::future::Future<Output = Result<McpRemoved, ProtocolError>> + Send;

    /// Handle `McpReload` — reload MCP servers from config.
    fn mcp_reload(
        &self,
    ) -> impl std::future::Future<Output = Result<McpReloaded, ProtocolError>> + Send;

    /// Handle `McpList` — list connected MCP servers.
    fn mcp_list(
        &self,
    ) -> impl std::future::Future<Output = Result<McpServerList, ProtocolError>> + Send;

    /// Handle `Ping` — keepalive.
    fn ping(&self) -> impl std::future::Future<Output = Result<(), ProtocolError>> + Send;
}
