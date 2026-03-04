//! Unix domain socket server — accept loop and per-connection message handler.

use crate::codec;
use protocol::message::{client::ClientMessage, server::ServerMessage};
use tokio::{
    net::{
        UnixListener,
        unix::{OwnedReadHalf, OwnedWriteHalf},
    },
    sync::{mpsc, oneshot},
};

/// Accept connections on the given `UnixListener` until shutdown is signalled.
///
/// Each connection is handled in a separate task. For each incoming
/// `ClientMessage`, calls `on_message(msg, reply_tx)` where `reply_tx` is
/// the per-connection sender for streaming `ServerMessage`s back. The caller
/// controls dispatch routing (DD#11).
pub async fn accept_loop<F>(
    listener: UnixListener,
    on_message: F,
    mut shutdown: oneshot::Receiver<()>,
) where
    F: Fn(ClientMessage, mpsc::UnboundedSender<ServerMessage>) + Clone + Send + 'static,
{
    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, _addr)) => {
                        let cb = on_message.clone();
                        tokio::spawn(async move {
                            handle_connection(stream, cb).await;
                        });
                    }
                    Err(e) => {
                        tracing::error!("failed to accept connection: {e}");
                    }
                }
            }
            _ = &mut shutdown => {
                tracing::info!("accept loop shutting down");
                break;
            }
        }
    }
}

/// Handle an established Unix domain socket connection.
async fn handle_connection<F>(stream: tokio::net::UnixStream, on_message: F)
where
    F: Fn(ClientMessage, mpsc::UnboundedSender<ServerMessage>),
{
    let (reader, writer) = stream.into_split();
    let (tx, rx) = mpsc::unbounded_channel::<ServerMessage>();

    // Sender task: forward ServerMessages to the socket.
    let send_task = tokio::spawn(sender_loop(writer, rx));

    // Receiver loop: process incoming ClientMessages.
    receiver_loop(reader, tx, on_message).await;

    // Clean up — dropping tx already happened in receiver_loop on exit,
    // which causes sender_loop to end.
    let _ = send_task.await;
}

/// Reads messages from the mpsc channel and writes them to the socket.
async fn sender_loop(mut writer: OwnedWriteHalf, mut rx: mpsc::UnboundedReceiver<ServerMessage>) {
    while let Some(msg) = rx.recv().await {
        if let Err(e) = codec::write_message(&mut writer, &msg).await {
            tracing::error!("failed to write message: {e}");
            break;
        }
    }
}

/// Reads client messages from the socket and dispatches via callback.
async fn receiver_loop<F>(
    mut reader: OwnedReadHalf,
    tx: mpsc::UnboundedSender<ServerMessage>,
    on_message: F,
) where
    F: Fn(ClientMessage, mpsc::UnboundedSender<ServerMessage>),
{
    loop {
        let client_msg: ClientMessage = match codec::read_message(&mut reader).await {
            Ok(msg) => msg,
            Err(codec::FrameError::ConnectionClosed) => break,
            Err(e) => {
                tracing::debug!("read error: {e}");
                break;
            }
        };

        on_message(client_msg, tx.clone());
    }
}
