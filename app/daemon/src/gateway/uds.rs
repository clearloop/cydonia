//! Unix domain socket server — accept loop and per-connection message handler.

use crate::gateway::Gateway;
use futures_util::StreamExt;
use protocol::api::Server;
use protocol::codec::{self, FrameError};
use protocol::message::client::ClientMessage;
use protocol::message::server::ServerMessage;
use protocol::message::{
    AgentInfoRequest, ClearSessionRequest, DownloadRequest, GetMemoryRequest, McpAddRequest,
    McpRemoveRequest, SendRequest, StreamRequest,
};
use tokio::net::UnixListener;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::{mpsc, oneshot};

/// Accept connections on the given `UnixListener` until shutdown is signalled.
pub async fn accept_loop(
    listener: UnixListener,
    state: Gateway,
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
async fn handle_connection(stream: tokio::net::UnixStream, state: Gateway) {
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

/// Convert a Server trait result into a ServerMessage.
fn result_to_msg<T: Into<ServerMessage>>(
    result: Result<T, protocol::error::ProtocolError>,
) -> ServerMessage {
    match result {
        Ok(resp) => resp.into(),
        Err(e) => e.into(),
    }
}

/// Reads client messages from the socket and dispatches them via Server trait.
async fn receiver_loop(
    mut reader: OwnedReadHalf,
    tx: mpsc::UnboundedSender<ServerMessage>,
    state: Gateway,
) {
    loop {
        let client_msg: ClientMessage = match codec::read_message(&mut reader).await {
            Ok(msg) => msg,
            Err(FrameError::ConnectionClosed) => break,
            Err(e) => {
                tracing::debug!("read error: {e}");
                break;
            }
        };

        match client_msg {
            ClientMessage::Send { agent, content } => {
                let msg = result_to_msg(state.send(SendRequest { agent, content }).await);
                let _ = tx.send(msg);
            }

            ClientMessage::Stream { agent, content } => {
                let stream = state.stream(StreamRequest { agent, content });
                futures_util::pin_mut!(stream);
                while let Some(result) = stream.next().await {
                    let msg = result_to_msg(result);
                    let _ = tx.send(msg);
                }
            }

            ClientMessage::ClearSession { agent } => {
                let msg = result_to_msg(state.clear_session(ClearSessionRequest { agent }).await);
                let _ = tx.send(msg);
            }

            ClientMessage::ListAgents => {
                let msg = result_to_msg(state.list_agents().await);
                let _ = tx.send(msg);
            }

            ClientMessage::AgentInfo { agent } => {
                let msg = result_to_msg(state.agent_info(AgentInfoRequest { agent }).await);
                let _ = tx.send(msg);
            }

            ClientMessage::ListMemory => {
                let msg = result_to_msg(state.list_memory().await);
                let _ = tx.send(msg);
            }

            ClientMessage::GetMemory { key } => {
                let msg = result_to_msg(state.get_memory(GetMemoryRequest { key }).await);
                let _ = tx.send(msg);
            }

            ClientMessage::Download { model } => {
                let stream = state.download(DownloadRequest { model });
                futures_util::pin_mut!(stream);
                while let Some(result) = stream.next().await {
                    let msg = result_to_msg(result);
                    let _ = tx.send(msg);
                }
            }

            ClientMessage::ReloadSkills => {
                let msg = result_to_msg(state.reload_skills().await);
                let _ = tx.send(msg);
            }

            ClientMessage::McpAdd {
                name,
                command,
                args,
                env,
            } => {
                let msg = result_to_msg(
                    state
                        .mcp_add(McpAddRequest {
                            name,
                            command,
                            args,
                            env,
                        })
                        .await,
                );
                let _ = tx.send(msg);
            }

            ClientMessage::McpRemove { name } => {
                let msg = result_to_msg(state.mcp_remove(McpRemoveRequest { name }).await);
                let _ = tx.send(msg);
            }

            ClientMessage::McpReload => {
                let msg = result_to_msg(state.mcp_reload().await);
                let _ = tx.send(msg);
            }

            ClientMessage::McpList => {
                let msg = result_to_msg(state.mcp_list().await);
                let _ = tx.send(msg);
            }

            ClientMessage::Ping => {
                let _ = tx.send(ServerMessage::Pong);
            }
        }
    }
}
