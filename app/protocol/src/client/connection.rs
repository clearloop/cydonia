//! Unix domain socket connection with `Client` trait implementation.

use crate::api::Client;
use crate::codec;
use crate::error::ProtocolError;
use crate::message::client::ClientMessage;
use crate::message::server::ServerMessage;
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
}

impl Client for Connection {
    async fn request(&mut self, msg: ClientMessage) -> Result<ServerMessage, ProtocolError> {
        codec::write_message(&mut self.writer, &msg)
            .await
            .map_err(|e| ProtocolError::new(0, e.to_string()))?;
        codec::read_message(&mut self.reader)
            .await
            .map_err(|e| ProtocolError::new(0, e.to_string()))
    }

    fn request_stream(
        &mut self,
        msg: ClientMessage,
    ) -> impl Stream<Item = Result<ServerMessage, ProtocolError>> + Send + '_ {
        async_stream::try_stream! {
            codec::write_message(&mut self.writer, &msg)
                .await
                .map_err(|e| ProtocolError::new(0, e.to_string()))?;

            loop {
                let server_msg: ServerMessage = codec::read_message(&mut self.reader)
                    .await
                    .map_err(|e| ProtocolError::new(0, e.to_string()))?;

                match &server_msg {
                    ServerMessage::Error { code, message } => {
                        Err(ProtocolError::new(*code, message.clone()))?;
                    }
                    _ => yield server_msg,
                }
            }
        }
    }
}
