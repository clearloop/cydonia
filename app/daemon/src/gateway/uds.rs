//! Unix domain socket server — accept loop and per-connection message handler.

use crate::config::McpServerConfig;
use crate::gateway::Gateway;
use futures_util::StreamExt;
use memory::Memory;
use protocol::codec::{self, FrameError};
use protocol::{AgentSummary, ClientMessage, McpServerSummary, ServerMessage};
use runtime::AgentDispatcher;
use tokio::net::UnixListener;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::{mpsc, oneshot};
use wcore::AgentEvent;
use wcore::model::Message;

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

/// Reads client messages from the socket and dispatches them.
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
                let Some(mut agent_instance) = state.runtime.take_agent(&agent).await else {
                    let _ = tx.send(ServerMessage::Error {
                        code: 404,
                        message: format!("agent '{agent}' not registered"),
                    });
                    continue;
                };

                agent_instance.push_message(Message::user(&content));
                let dispatcher = AgentDispatcher {
                    hook: state.runtime.hook(),
                    agent: &agent,
                };

                let response = agent_instance.run(&dispatcher).await;
                let content = response.final_response.unwrap_or_default();
                let _ = tx.send(ServerMessage::Response {
                    agent: agent.clone(),
                    content,
                });

                state.runtime.put_agent(agent_instance).await;
            }

            ClientMessage::Stream { agent, content } => {
                let Some(mut agent_instance) = state.runtime.take_agent(&agent).await else {
                    let _ = tx.send(ServerMessage::Error {
                        code: 404,
                        message: format!("agent '{agent}' not registered"),
                    });
                    continue;
                };

                let _ = tx.send(ServerMessage::StreamStart {
                    agent: agent.clone(),
                });

                agent_instance.push_message(Message::user(&content));
                {
                    let dispatcher = AgentDispatcher {
                        hook: state.runtime.hook(),
                        agent: &agent,
                    };
                    let stream = agent_instance.run_stream(&dispatcher);
                    futures_util::pin_mut!(stream);
                    while let Some(event) = stream.next().await {
                        match event {
                            AgentEvent::TextDelta(text) => {
                                let _ = tx.send(ServerMessage::StreamChunk { content: text });
                            }
                            AgentEvent::Done(_) => break,
                            _ => {}
                        }
                    }
                }

                let _ = tx.send(ServerMessage::StreamEnd { agent });
                state.runtime.put_agent(agent_instance).await;
            }

            ClientMessage::Download { model } => {
                let _ = tx.send(ServerMessage::DownloadStart {
                    model: model.clone(),
                });

                let (dtx, mut drx) = mpsc::unbounded_channel();
                let model_str = model.to_string();
                let endpoint = state.hf_endpoint.clone();
                let download_handle = tokio::spawn(async move {
                    model::local::download::download_model(&model_str, &endpoint, dtx).await
                });

                while let Some(event) = drx.recv().await {
                    let msg = match event {
                        model::local::download::DownloadEvent::FileStart { filename, size } => {
                            ServerMessage::DownloadFileStart { filename, size }
                        }
                        model::local::download::DownloadEvent::Progress { bytes } => {
                            ServerMessage::DownloadProgress { bytes }
                        }
                        model::local::download::DownloadEvent::FileEnd { filename } => {
                            ServerMessage::DownloadFileEnd { filename }
                        }
                    };
                    let _ = tx.send(msg);
                }

                match download_handle.await {
                    Ok(Ok(())) => {
                        let _ = tx.send(ServerMessage::DownloadEnd { model });
                    }
                    Ok(Err(e)) => {
                        let _ = tx.send(ServerMessage::Error {
                            code: 500,
                            message: format!("download failed: {e}"),
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(ServerMessage::Error {
                            code: 500,
                            message: format!("download task panicked: {e}"),
                        });
                    }
                }
            }

            ClientMessage::ClearSession { agent } => {
                state.runtime.clear_session(&agent).await;
                let _ = tx.send(ServerMessage::SessionCleared { agent });
            }

            ClientMessage::ListAgents => {
                let agents = state
                    .runtime
                    .agents()
                    .await
                    .into_iter()
                    .map(|a| AgentSummary {
                        name: a.name.clone(),
                        description: a.description.clone(),
                    })
                    .collect();
                let _ = tx.send(ServerMessage::AgentList { agents });
            }

            ClientMessage::AgentInfo { agent } => match state.runtime.agent(&agent).await {
                Some(a) => {
                    let _ = tx.send(ServerMessage::AgentDetail {
                        name: a.name.clone(),
                        description: a.description.clone(),
                        tools: a.tools.to_vec(),
                        skill_tags: a.skill_tags.to_vec(),
                        system_prompt: a.system_prompt.clone(),
                    });
                }
                None => {
                    let _ = tx.send(ServerMessage::Error {
                        code: 404,
                        message: format!("agent not found: {agent}"),
                    });
                }
            },

            ClientMessage::ListMemory => {
                let entries = state.runtime.hook().memory().entries();
                let _ = tx.send(ServerMessage::MemoryList { entries });
            }

            ClientMessage::GetMemory { key } => {
                let value = state.runtime.hook().memory().get(&key);
                let _ = tx.send(ServerMessage::MemoryEntry { key, value });
            }

            ClientMessage::ReloadSkills => match state.runtime.hook().skills().reload().await {
                Ok(count) => {
                    tracing::info!("reloaded {count} skill(s)");
                    let _ = tx.send(ServerMessage::SkillsReloaded { count });
                }
                Err(e) => {
                    let _ = tx.send(ServerMessage::Error {
                        code: 500,
                        message: format!("failed to reload skills: {e}"),
                    });
                }
            },

            ClientMessage::McpAdd {
                name,
                command,
                args,
                env,
            } => {
                let config = McpServerConfig {
                    name: name.clone(),
                    command,
                    args,
                    env,
                    auto_restart: true,
                };
                match state.runtime.hook().mcp().add(config).await {
                    Ok(tools) => {
                        let _ = tx.send(ServerMessage::McpAdded { name, tools });
                    }
                    Err(e) => {
                        let _ = tx.send(ServerMessage::Error {
                            code: 500,
                            message: format!("failed to add MCP server: {e}"),
                        });
                    }
                }
            }

            ClientMessage::McpRemove { name } => {
                match state.runtime.hook().mcp().remove(&name).await {
                    Ok(tools) => {
                        let _ = tx.send(ServerMessage::McpRemoved { name, tools });
                    }
                    Err(e) => {
                        let _ = tx.send(ServerMessage::Error {
                            code: 500,
                            message: format!("failed to remove MCP server: {e}"),
                        });
                    }
                }
            }

            ClientMessage::McpReload => match state.runtime.hook().mcp().reload().await {
                Ok(servers) => {
                    let servers = servers
                        .into_iter()
                        .map(|(name, tools)| McpServerSummary { name, tools })
                        .collect();
                    let _ = tx.send(ServerMessage::McpReloaded { servers });
                }
                Err(e) => {
                    let _ = tx.send(ServerMessage::Error {
                        code: 500,
                        message: format!("MCP reload failed: {e}"),
                    });
                }
            },

            ClientMessage::McpList => {
                let servers = state
                    .runtime
                    .hook()
                    .mcp()
                    .list()
                    .await
                    .into_iter()
                    .map(|(name, tools)| McpServerSummary { name, tools })
                    .collect();
                let _ = tx.send(ServerMessage::McpServerList { servers });
            }

            ClientMessage::Ping => {
                let _ = tx.send(ServerMessage::Pong);
            }
        }
    }
}
