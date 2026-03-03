//! Unix domain socket server — accept loop and per-connection message handler.

use crate::codec;
use futures_util::StreamExt;
use protocol::api::Server;
use protocol::message::client::ClientMessage;
use protocol::message::server::ServerMessage;
use tokio::net::UnixListener;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::{mpsc, oneshot};

/// Accept connections on the given `UnixListener` until shutdown is signalled.
///
/// Each connection is handled in a separate task. The `state` must implement
/// [`Server`] and be cheaply cloneable (typically via `Arc` internals).
pub async fn accept_loop<S: Server + Clone + Send + 'static>(
    listener: UnixListener,
    state: S,
    mut shutdown: oneshot::Receiver<()>,
) {
    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, _addr)) => {
                        let state = state.clone();
                        tokio::spawn(async move {
                            handle_connection(stream, state).await;
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
async fn handle_connection<S: Server>(stream: tokio::net::UnixStream, state: S) {
    let (reader, writer) = stream.into_split();
    let (tx, rx) = mpsc::unbounded_channel::<ServerMessage>();

    // Sender task: forward ServerMessages to the socket.
    let send_task = tokio::spawn(sender_loop(writer, rx));

    // Receiver loop: process incoming ClientMessages.
    receiver_loop(reader, tx, state).await;

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

/// Reads client messages from the socket and dispatches them via Server trait.
async fn receiver_loop<S: Server>(
    mut reader: OwnedReadHalf,
    tx: mpsc::UnboundedSender<ServerMessage>,
    state: S,
) {
    loop {
        let client_msg: ClientMessage = match codec::read_message(&mut reader).await {
            Ok(msg) => msg,
            Err(codec::FrameError::ConnectionClosed) => break,
            Err(e) => {
                tracing::debug!("read error: {e}");
                break;
            }
        };

        let stream = state.dispatch(client_msg);
        tokio::pin!(stream);
        while let Some(server_msg) = stream.next().await {
            let _ = tx.send(server_msg);
        }
    }
}
