//! Server trait — one async method per protocol operation.

use crate::protocol::message::{
    DownloadEvent, DownloadRequest, HubAction, HubEvent, SendRequest, SendResponse, StreamEvent,
    StreamRequest,
    client::ClientMessage,
    server::{ServerMessage, SessionInfo},
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

    /// Handle `Download` — download model files with progress.
    fn download(&self, req: DownloadRequest) -> impl Stream<Item = Result<DownloadEvent>> + Send;

    /// Handle `Ping` — keepalive.
    fn ping(&self) -> impl std::future::Future<Output = Result<()>> + Send;

    /// Handle `Hub` — install or uninstall a hub package.
    fn hub(
        &self,
        package: compact_str::CompactString,
        action: HubAction,
    ) -> impl Stream<Item = Result<HubEvent>> + Send;

    /// Handle `Sessions` — list active sessions.
    fn list_sessions(&self) -> impl std::future::Future<Output = Result<Vec<SessionInfo>>> + Send;

    /// Handle `Kill` — close a session by ID.
    fn kill_session(&self, session: u64) -> impl std::future::Future<Output = Result<bool>> + Send;

    /// Dispatch a `ClientMessage` to the appropriate handler method.
    ///
    /// Returns a stream of `ServerMessage`s. Request-response operations
    /// yield exactly one message; streaming operations yield many.
    fn dispatch(&self, msg: ClientMessage) -> impl Stream<Item = ServerMessage> + Send + '_ {
        async_stream::stream! {
            match msg {
                ClientMessage::Send { agent, content, session } => {
                    yield result_to_msg(self.send(SendRequest { agent, content, session }).await);
                }
                ClientMessage::Stream { agent, content, session } => {
                    let s = self.stream(StreamRequest { agent, content, session });
                    tokio::pin!(s);
                    while let Some(result) = s.next().await {
                        yield result_to_msg(result);
                    }
                }
                ClientMessage::Download { model } => {
                    let s = self.download(DownloadRequest { model });
                    tokio::pin!(s);
                    while let Some(result) = s.next().await {
                        yield result_to_msg(result);
                    }
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
                ClientMessage::Hub { package, action } => {
                    let s = self.hub(package, action);
                    tokio::pin!(s);
                    while let Some(result) = s.next().await {
                        yield result_to_msg(result);
                    }
                }
                ClientMessage::Sessions => {
                    yield match self.list_sessions().await {
                        Ok(sessions) => ServerMessage::Sessions(sessions),
                        Err(e) => ServerMessage::Error {
                            code: 500,
                            message: e.to_string(),
                        },
                    };
                }
                ClientMessage::Kill { session } => {
                    yield match self.kill_session(session).await {
                        Ok(true) => ServerMessage::Pong,
                        Ok(false) => ServerMessage::Error {
                            code: 404,
                            message: format!("session {session} not found"),
                        },
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
