//! Telegram bot command parsing and dispatch.
//!
//! Detects `/cmd` prefixed messages and maps them to daemon operations
//! (hub install/uninstall, model download), streaming progress back to chat.

use crate::channel::ChannelSender;
use crate::message::{ChannelMessage, Platform};
use compact_str::CompactString;
use futures_util::StreamExt;
use socket::{ClientConfig, WalrusClient};
use std::path::PathBuf;
use wcore::protocol::message::{DownloadEvent, DownloadRequest, HubAction, HubEvent, HubRequest};

/// A parsed bot command from a `/cmd` message.
pub(crate) enum BotCommand {
    HubInstall { package: String },
    HubUninstall { package: String },
    ModelDownload { model: String },
}

/// Parse a message content string into a `BotCommand`.
///
/// Returns `None` for non-`/` messages or unrecognised commands.
pub(crate) fn parse_command(content: &str) -> Option<BotCommand> {
    let mut parts = content.split_whitespace();
    let first = parts.next()?;
    if !first.starts_with('/') {
        return None;
    }
    let second = parts.next()?;
    let arg = parts.next().map(str::to_owned);

    match (first, second) {
        ("/hub", "install") => Some(BotCommand::HubInstall {
            package: arg.unwrap_or_default(),
        }),
        ("/hub", "uninstall") => Some(BotCommand::HubUninstall {
            package: arg.unwrap_or_default(),
        }),
        ("/model", "download") => Some(BotCommand::ModelDownload {
            model: arg.unwrap_or_default(),
        }),
        _ => None,
    }
}

/// Execute a bot command, streaming progress messages back to the originating chat.
pub(crate) async fn dispatch_command(
    cmd: BotCommand,
    socket_path: PathBuf,
    sender: ChannelSender,
    platform: Platform,
    channel_id: CompactString,
) {
    use wcore::protocol::api::Client;

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
            let stream = connection.hub(HubRequest {
                package: CompactString::from(&package),
                action: HubAction::Install,
            });
            futures_util::pin_mut!(stream);
            while let Some(result) = stream.next().await {
                match result {
                    Ok(HubEvent::Start { package }) => {
                        send_text(
                            &sender,
                            platform,
                            &channel_id,
                            format!("Starting hub operation for {package}..."),
                        )
                        .await;
                    }
                    Ok(HubEvent::Step { message }) => {
                        send_text(&sender, platform, &channel_id, format!("  {message}")).await;
                    }
                    Ok(HubEvent::End { package }) => {
                        send_text(&sender, platform, &channel_id, format!("Done: {package}")).await;
                    }
                    Err(e) => {
                        tracing::warn!("hub event error: {e}");
                    }
                }
            }
        }
        BotCommand::HubUninstall { package } => {
            let stream = connection.hub(HubRequest {
                package: CompactString::from(&package),
                action: HubAction::Uninstall,
            });
            futures_util::pin_mut!(stream);
            while let Some(result) = stream.next().await {
                match result {
                    Ok(HubEvent::Start { package }) => {
                        send_text(
                            &sender,
                            platform,
                            &channel_id,
                            format!("Starting hub operation for {package}..."),
                        )
                        .await;
                    }
                    Ok(HubEvent::Step { message }) => {
                        send_text(&sender, platform, &channel_id, format!("  {message}")).await;
                    }
                    Ok(HubEvent::End { package }) => {
                        send_text(&sender, platform, &channel_id, format!("Done: {package}")).await;
                    }
                    Err(e) => {
                        tracing::warn!("hub event error: {e}");
                    }
                }
            }
        }
        BotCommand::ModelDownload { model } => {
            let stream = connection.download(DownloadRequest {
                model: CompactString::from(&model),
            });
            futures_util::pin_mut!(stream);
            while let Some(result) = stream.next().await {
                match result {
                    Ok(DownloadEvent::Start { model }) => {
                        send_text(
                            &sender,
                            platform,
                            &channel_id,
                            format!("Downloading {model}..."),
                        )
                        .await;
                    }
                    Ok(DownloadEvent::FileStart { filename, .. }) => {
                        send_text(
                            &sender,
                            platform,
                            &channel_id,
                            format!("  {filename} starting..."),
                        )
                        .await;
                    }
                    Ok(DownloadEvent::Progress { .. }) => {
                        // Too noisy for chat — skip.
                    }
                    Ok(DownloadEvent::FileEnd { filename, .. }) => {
                        send_text(&sender, platform, &channel_id, format!("  {filename} done"))
                            .await;
                    }
                    Ok(DownloadEvent::End { model }) => {
                        send_text(
                            &sender,
                            platform,
                            &channel_id,
                            format!("Download complete: {model}"),
                        )
                        .await;
                    }
                    Err(e) => {
                        tracing::warn!("download event error: {e}");
                    }
                }
            }
        }
    }
}

/// Send a plain-text message back to the originating chat.
async fn send_text(
    sender: &ChannelSender,
    platform: Platform,
    channel_id: &CompactString,
    content: String,
) {
    let msg = ChannelMessage {
        platform,
        channel_id: channel_id.clone(),
        sender_id: CompactString::default(),
        content,
        attachments: Vec::new(),
        reply_to: None,
        timestamp: 0,
    };
    if let Err(e) = sender.send(msg).await {
        tracing::warn!("failed to send bot command reply: {e}");
    }
}
