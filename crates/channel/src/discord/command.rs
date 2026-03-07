//! Discord bot command dispatch.
//!
//! Executes parsed bot commands (hub install/uninstall, model download)
//! by streaming progress back to the originating Discord channel.

use crate::command::BotCommand;
use compact_str::CompactString;
use futures_util::StreamExt;
use serenity::model::id::ChannelId;
use socket::{ClientConfig, Connection, WalrusClient};
use std::path::PathBuf;
use std::sync::Arc;
use wcore::protocol::api::Client;
use wcore::protocol::message::{DownloadEvent, DownloadRequest, HubAction, HubEvent, HubRequest};

/// Execute a bot command, streaming progress messages back to the originating channel.
pub(crate) async fn dispatch_command(
    cmd: BotCommand,
    socket_path: PathBuf,
    http: Arc<serenity::http::Http>,
    channel_id: ChannelId,
) {
    let config = ClientConfig { socket_path };
    let client = WalrusClient::new(config);
    let mut connection = match client.connect().await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("bot command: failed to connect to daemon: {e}");
            return;
        }
    };

    match cmd {
        BotCommand::HubInstall { package } => {
            run_hub(
                &mut connection,
                &http,
                channel_id,
                package,
                HubAction::Install,
            )
            .await;
        }
        BotCommand::HubUninstall { package } => {
            run_hub(
                &mut connection,
                &http,
                channel_id,
                package,
                HubAction::Uninstall,
            )
            .await;
        }
        BotCommand::ModelDownload { model } => {
            let stream = connection.download(DownloadRequest {
                model: CompactString::from(&model),
            });
            futures_util::pin_mut!(stream);
            while let Some(result) = stream.next().await {
                match result {
                    Ok(DownloadEvent::Start { model }) => {
                        send_text(&http, channel_id, format!("Downloading {model}...")).await;
                    }
                    Ok(DownloadEvent::FileStart { filename, .. }) => {
                        send_text(&http, channel_id, format!("  {filename} starting...")).await;
                    }
                    Ok(DownloadEvent::Progress { .. }) => {}
                    Ok(DownloadEvent::FileEnd { filename, .. }) => {
                        send_text(&http, channel_id, format!("  {filename} done")).await;
                    }
                    Ok(DownloadEvent::End { model }) => {
                        send_text(&http, channel_id, format!("Download complete: {model}")).await;
                    }
                    Err(e) => {
                        tracing::warn!("download event error: {e}");
                    }
                }
            }
        }
    }
}

/// Stream a hub install/uninstall operation and send progress to the channel.
async fn run_hub(
    connection: &mut Connection,
    http: &Arc<serenity::http::Http>,
    channel_id: ChannelId,
    package: String,
    action: HubAction,
) {
    let stream = connection.hub(HubRequest {
        package: CompactString::from(&package),
        action,
    });
    futures_util::pin_mut!(stream);
    while let Some(result) = stream.next().await {
        match result {
            Ok(HubEvent::Start { package }) => {
                send_text(
                    http,
                    channel_id,
                    format!("Starting hub operation for {package}..."),
                )
                .await;
            }
            Ok(HubEvent::Step { message }) => {
                send_text(http, channel_id, format!("  {message}")).await;
            }
            Ok(HubEvent::End { package }) => {
                send_text(http, channel_id, format!("Done: {package}")).await;
            }
            Err(e) => {
                tracing::warn!("hub event error: {e}");
            }
        }
    }
}

/// Send a plain-text message to the channel.
async fn send_text(http: &Arc<serenity::http::Http>, channel_id: ChannelId, content: String) {
    if let Err(e) = channel_id.say(http, content).await {
        tracing::warn!("failed to send bot command reply: {e}");
    }
}
