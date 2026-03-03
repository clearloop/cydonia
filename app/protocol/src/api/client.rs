//! Client trait — one async method per protocol operation.

use crate::error::ProtocolError;
use crate::message::{
    AgentDetail, AgentInfoRequest, AgentList, ClearSessionRequest, DownloadEvent, DownloadRequest,
    GetMemoryRequest, McpAddRequest, McpAdded, McpReloaded, McpRemoveRequest, McpRemoved,
    McpServerList, MemoryEntry, MemoryList, SendRequest, SendResponse, SessionCleared,
    SkillsReloaded, StreamEvent, StreamRequest,
};
use futures_core::Stream;

/// Client-side protocol interface.
///
/// Each method corresponds to one `ClientMessage` variant. Implementations
/// send typed request structs and receive typed responses — no enum matching
/// required. Streaming operations return `impl Stream`.
pub trait Client {
    /// Send a message to an agent and receive a complete response.
    fn send(
        &mut self,
        req: SendRequest,
    ) -> impl std::future::Future<Output = Result<SendResponse, ProtocolError>> + Send;

    /// Send a message to an agent and receive a streamed response.
    fn stream(
        &mut self,
        req: StreamRequest,
    ) -> impl Stream<Item = Result<StreamEvent, ProtocolError>> + Send + '_;

    /// Clear the session history for an agent.
    fn clear_session(
        &mut self,
        req: ClearSessionRequest,
    ) -> impl std::future::Future<Output = Result<SessionCleared, ProtocolError>> + Send;

    /// List all registered agents.
    fn list_agents(
        &mut self,
    ) -> impl std::future::Future<Output = Result<AgentList, ProtocolError>> + Send;

    /// Get detailed info for a specific agent.
    fn agent_info(
        &mut self,
        req: AgentInfoRequest,
    ) -> impl std::future::Future<Output = Result<AgentDetail, ProtocolError>> + Send;

    /// List all memory entries.
    fn list_memory(
        &mut self,
    ) -> impl std::future::Future<Output = Result<MemoryList, ProtocolError>> + Send;

    /// Get a specific memory entry by key.
    fn get_memory(
        &mut self,
        req: GetMemoryRequest,
    ) -> impl std::future::Future<Output = Result<MemoryEntry, ProtocolError>> + Send;

    /// Download a model's files with progress reporting.
    fn download(
        &mut self,
        req: DownloadRequest,
    ) -> impl Stream<Item = Result<DownloadEvent, ProtocolError>> + Send + '_;

    /// Reload skills from disk.
    fn reload_skills(
        &mut self,
    ) -> impl std::future::Future<Output = Result<SkillsReloaded, ProtocolError>> + Send;

    /// Add an MCP server.
    fn mcp_add(
        &mut self,
        req: McpAddRequest,
    ) -> impl std::future::Future<Output = Result<McpAdded, ProtocolError>> + Send;

    /// Remove an MCP server.
    fn mcp_remove(
        &mut self,
        req: McpRemoveRequest,
    ) -> impl std::future::Future<Output = Result<McpRemoved, ProtocolError>> + Send;

    /// Reload MCP servers from config.
    fn mcp_reload(
        &mut self,
    ) -> impl std::future::Future<Output = Result<McpReloaded, ProtocolError>> + Send;

    /// List connected MCP servers.
    fn mcp_list(
        &mut self,
    ) -> impl std::future::Future<Output = Result<McpServerList, ProtocolError>> + Send;

    /// Ping the server (keepalive).
    fn ping(&mut self) -> impl std::future::Future<Output = Result<(), ProtocolError>> + Send;
}
