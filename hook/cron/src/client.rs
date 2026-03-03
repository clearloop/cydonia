//! In-process protocol client — wraps a `Server` impl for direct dispatch.

use anyhow::Result;
use futures_util::StreamExt;
use protocol::api::{Client, Server};
use protocol::message::client::ClientMessage;
use protocol::message::server::ServerMessage;

/// In-process protocol client that delegates to a `Server` impl.
///
/// No socket overhead — calls `Server::dispatch` directly and collects
/// the first response from the returned stream. Intended for request-response
/// operations (`Send`). Streaming messages will be silently truncated.
pub struct CronClient<S> {
    server: S,
}

impl<S: Server> CronClient<S> {
    /// Wrap a server impl as an in-process client.
    pub fn new(server: S) -> Self {
        Self { server }
    }
}

impl<S: Server + Send> Client for CronClient<S> {
    async fn request(&mut self, msg: ClientMessage) -> Result<ServerMessage> {
        let stream = self.server.dispatch(msg);
        futures_util::pin_mut!(stream);
        stream
            .next()
            .await
            .ok_or_else(|| anyhow::anyhow!("server returned empty response"))
    }

    fn request_stream(
        &mut self,
        msg: ClientMessage,
    ) -> impl futures_core::Stream<Item = Result<ServerMessage>> + Send + '_ {
        self.server.dispatch(msg).map(Ok)
    }
}
