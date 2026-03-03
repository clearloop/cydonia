//! Server trait — one async method per protocol operation.

use crate::message::client::ClientMessage;
use crate::message::server::ServerMessage;
use crate::message::{
    AgentDetail, AgentInfoRequest, AgentList, ClearSessionRequest, DownloadEvent, DownloadRequest,
    GetMemoryRequest, McpAddRequest, McpAdded, McpReloaded, McpRemoveRequest, McpRemoved,
    McpServerList, MemoryEntry, MemoryList, SendRequest, SendResponse, SessionCleared,
    SkillsReloaded, StreamEvent, StreamRequest,
};
use anyhow::Result;
use futures_core::Stream;
use futures_util::StreamExt;

/// Server-side protocol handler.
///
/// Each method corresponds to one `ClientMessage` variant. Implementations
/// receive typed request structs and return typed responses — no enum matching
/// required. Streaming operations return `impl Stream`.
///
/// The provided [`dispatch`](Server::dispatch) method routes a raw
/// `ClientMessage` to the appropriate handler, returning a stream of
/// `ServerMessage`s.
pub trait Server: Sync {
    /// Handle `Send` — run agent and return complete response.
    fn send(
        &self,
        req: SendRequest,
    ) -> impl std::future::Future<Output = Result<SendResponse>> + Send;

    /// Handle `Stream` — run agent and stream response events.
    fn stream(&self, req: StreamRequest) -> impl Stream<Item = Result<StreamEvent>> + Send;

    /// Handle `ClearSession` — clear agent history.
    fn clear_session(
        &self,
        req: ClearSessionRequest,
    ) -> impl std::future::Future<Output = Result<SessionCleared>> + Send;

    /// Handle `ListAgents` — list all registered agents.
    fn list_agents(&self) -> impl std::future::Future<Output = Result<AgentList>> + Send;

    /// Handle `AgentInfo` — get agent details.
    fn agent_info(
        &self,
        req: AgentInfoRequest,
    ) -> impl std::future::Future<Output = Result<AgentDetail>> + Send;

    /// Handle `ListMemory` — list all memory entries.
    fn list_memory(&self) -> impl std::future::Future<Output = Result<MemoryList>> + Send;

    /// Handle `GetMemory` — get a memory entry by key.
    fn get_memory(
        &self,
        req: GetMemoryRequest,
    ) -> impl std::future::Future<Output = Result<MemoryEntry>> + Send;

    /// Handle `Download` — download model files with progress.
    fn download(&self, req: DownloadRequest) -> impl Stream<Item = Result<DownloadEvent>> + Send;

    /// Handle `ReloadSkills` — reload skills from disk.
    fn reload_skills(&self) -> impl std::future::Future<Output = Result<SkillsReloaded>> + Send;

    /// Handle `McpAdd` — add an MCP server.
    fn mcp_add(
        &self,
        req: McpAddRequest,
    ) -> impl std::future::Future<Output = Result<McpAdded>> + Send;

    /// Handle `McpRemove` — remove an MCP server.
    fn mcp_remove(
        &self,
        req: McpRemoveRequest,
    ) -> impl std::future::Future<Output = Result<McpRemoved>> + Send;

    /// Handle `McpReload` — reload MCP servers from config.
    fn mcp_reload(&self) -> impl std::future::Future<Output = Result<McpReloaded>> + Send;

    /// Handle `McpList` — list connected MCP servers.
    fn mcp_list(&self) -> impl std::future::Future<Output = Result<McpServerList>> + Send;

    /// Handle `Ping` — keepalive.
    fn ping(&self) -> impl std::future::Future<Output = Result<()>> + Send;

    /// Dispatch a `ClientMessage` to the appropriate handler method.
    ///
    /// Returns a stream of `ServerMessage`s. Request-response operations
    /// yield exactly one message; streaming operations yield many.
    fn dispatch(&self, msg: ClientMessage) -> impl Stream<Item = ServerMessage> + Send + '_ {
        async_stream::stream! {
            match msg {
                ClientMessage::Send { agent, content } => {
                    yield result_to_msg(self.send(SendRequest { agent, content }).await);
                }
                ClientMessage::Stream { agent, content } => {
                    let s = self.stream(StreamRequest { agent, content });
                    tokio::pin!(s);
                    while let Some(result) = s.next().await {
                        yield result_to_msg(result);
                    }
                }
                ClientMessage::ClearSession { agent } => {
                    yield result_to_msg(
                        self.clear_session(ClearSessionRequest { agent }).await,
                    );
                }
                ClientMessage::ListAgents => {
                    yield result_to_msg(self.list_agents().await);
                }
                ClientMessage::AgentInfo { agent } => {
                    yield result_to_msg(self.agent_info(AgentInfoRequest { agent }).await);
                }
                ClientMessage::ListMemory => {
                    yield result_to_msg(self.list_memory().await);
                }
                ClientMessage::GetMemory { key } => {
                    yield result_to_msg(self.get_memory(GetMemoryRequest { key }).await);
                }
                ClientMessage::Download { model } => {
                    let s = self.download(DownloadRequest { model });
                    tokio::pin!(s);
                    while let Some(result) = s.next().await {
                        yield result_to_msg(result);
                    }
                }
                ClientMessage::ReloadSkills => {
                    yield result_to_msg(self.reload_skills().await);
                }
                ClientMessage::McpAdd { name, command, args, env } => {
                    yield result_to_msg(
                        self.mcp_add(McpAddRequest { name, command, args, env }).await,
                    );
                }
                ClientMessage::McpRemove { name } => {
                    yield result_to_msg(
                        self.mcp_remove(McpRemoveRequest { name }).await,
                    );
                }
                ClientMessage::McpReload => {
                    yield result_to_msg(self.mcp_reload().await);
                }
                ClientMessage::McpList => {
                    yield result_to_msg(self.mcp_list().await);
                }
                ClientMessage::Ping => {
                    yield match self.ping().await {
                        Ok(()) => ServerMessage::Pong,
                        Err(e) => ServerMessage::Error {
                            code: 500,
                            message: e.to_string(),
                        },
                    };
                }
            }
        }
    }
}

/// Convert a typed `Result` into a `ServerMessage`.
fn result_to_msg<T: Into<ServerMessage>>(result: Result<T>) -> ServerMessage {
    match result {
        Ok(resp) => resp.into(),
        Err(e) => ServerMessage::Error {
            code: 500,
            message: e.to_string(),
        },
    }
}
