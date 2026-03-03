//! Unix domain socket connection with `Client` trait implementation.

use crate::api::Client;
use crate::codec;
use crate::error::ProtocolError;
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
use std::path::Path;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

/// An established Unix domain socket connection to a walrus-gateway.
///
/// Not Clone — one connection per session. Use [`super::WalrusClient::connect`]
/// to create a connection.
pub struct Connection {
    reader: OwnedReadHalf,
    writer: OwnedWriteHalf,
}

impl Connection {
    /// Connect to a gateway at the given socket path.
    pub async fn connect(socket_path: &Path) -> Result<Self> {
        let stream = tokio::net::UnixStream::connect(socket_path).await?;
        tracing::debug!("connected to {}", socket_path.display());
        let (reader, writer) = stream.into_split();
        Ok(Self { reader, writer })
    }

    /// Close the connection by dropping both halves.
    pub fn close(self) {
        drop(self);
    }

    /// Write a `ClientMessage` and read back a `ServerMessage`.
    async fn request(&mut self, msg: ClientMessage) -> Result<ServerMessage, ProtocolError> {
        codec::write_message(&mut self.writer, &msg)
            .await
            .map_err(|e| ProtocolError::new(0, e.to_string()))?;
        codec::read_message(&mut self.reader)
            .await
            .map_err(|e| ProtocolError::new(0, e.to_string()))
    }

    /// Write a `ClientMessage` and read back a typed response via `TryFrom`.
    async fn request_typed<T>(&mut self, msg: ClientMessage) -> Result<T, ProtocolError>
    where
        T: TryFrom<ServerMessage, Error = ProtocolError>,
    {
        let server_msg = self.request(msg).await?;
        T::try_from(server_msg)
    }
}

impl Client for Connection {
    fn send(
        &mut self,
        req: SendRequest,
    ) -> impl std::future::Future<Output = Result<SendResponse, ProtocolError>> + Send {
        self.request_typed(req.into())
    }

    fn stream(
        &mut self,
        req: StreamRequest,
    ) -> impl Stream<Item = Result<StreamEvent, ProtocolError>> + Send + '_ {
        async_stream::try_stream! {
            let msg: ClientMessage = req.into();
            codec::write_message(&mut self.writer, &msg)
                .await
                .map_err(|e| ProtocolError::new(0, e.to_string()))?;

            loop {
                let server_msg: ServerMessage = codec::read_message(&mut self.reader)
                    .await
                    .map_err(|e| ProtocolError::new(0, e.to_string()))?;

                match &server_msg {
                    ServerMessage::StreamEnd { .. } => break,
                    ServerMessage::Error { code, message } => {
                        Err(ProtocolError::new(*code, message.clone()))?;
                    }
                    _ => {
                        yield StreamEvent::try_from(server_msg)?;
                    }
                }
            }
        }
    }

    fn clear_session(
        &mut self,
        req: ClearSessionRequest,
    ) -> impl std::future::Future<Output = Result<SessionCleared, ProtocolError>> + Send {
        self.request_typed(req.into())
    }

    fn list_agents(
        &mut self,
    ) -> impl std::future::Future<Output = Result<AgentList, ProtocolError>> + Send {
        self.request_typed(ClientMessage::ListAgents)
    }

    fn agent_info(
        &mut self,
        req: AgentInfoRequest,
    ) -> impl std::future::Future<Output = Result<AgentDetail, ProtocolError>> + Send {
        self.request_typed(req.into())
    }

    fn list_memory(
        &mut self,
    ) -> impl std::future::Future<Output = Result<MemoryList, ProtocolError>> + Send {
        self.request_typed(ClientMessage::ListMemory)
    }

    fn get_memory(
        &mut self,
        req: GetMemoryRequest,
    ) -> impl std::future::Future<Output = Result<MemoryEntry, ProtocolError>> + Send {
        self.request_typed(req.into())
    }

    fn download(
        &mut self,
        req: DownloadRequest,
    ) -> impl Stream<Item = Result<DownloadEvent, ProtocolError>> + Send + '_ {
        async_stream::try_stream! {
            let msg: ClientMessage = req.into();
            codec::write_message(&mut self.writer, &msg)
                .await
                .map_err(|e| ProtocolError::new(0, e.to_string()))?;

            loop {
                let server_msg: ServerMessage = codec::read_message(&mut self.reader)
                    .await
                    .map_err(|e| ProtocolError::new(0, e.to_string()))?;

                match &server_msg {
                    ServerMessage::DownloadEnd { .. } => {
                        yield DownloadEvent::try_from(server_msg)?;
                        break;
                    }
                    ServerMessage::Error { code, message } => {
                        Err(ProtocolError::new(*code, message.clone()))?;
                    }
                    _ => {
                        yield DownloadEvent::try_from(server_msg)?;
                    }
                }
            }
        }
    }

    fn reload_skills(
        &mut self,
    ) -> impl std::future::Future<Output = Result<SkillsReloaded, ProtocolError>> + Send {
        self.request_typed(ClientMessage::ReloadSkills)
    }

    fn mcp_add(
        &mut self,
        req: McpAddRequest,
    ) -> impl std::future::Future<Output = Result<McpAdded, ProtocolError>> + Send {
        self.request_typed(req.into())
    }

    fn mcp_remove(
        &mut self,
        req: McpRemoveRequest,
    ) -> impl std::future::Future<Output = Result<McpRemoved, ProtocolError>> + Send {
        self.request_typed(req.into())
    }

    fn mcp_reload(
        &mut self,
    ) -> impl std::future::Future<Output = Result<McpReloaded, ProtocolError>> + Send {
        self.request_typed(ClientMessage::McpReload)
    }

    fn mcp_list(
        &mut self,
    ) -> impl std::future::Future<Output = Result<McpServerList, ProtocolError>> + Send {
        self.request_typed(ClientMessage::McpList)
    }

    async fn ping(&mut self) -> Result<(), ProtocolError> {
        let server_msg = self.request(ClientMessage::Ping).await?;
        match server_msg {
            ServerMessage::Pong => Ok(()),
            other => {
                let e = match other {
                    ServerMessage::Error { code, message } => ProtocolError { code, message },
                    _ => ProtocolError::new(0, format!("unexpected response: {other:?}")),
                };
                Err(e)
            }
        }
    }
}
