//! Telegram bot command parsing and dispatch.
//!
//! Detects `/cmd` prefixed messages and maps them to daemon operations
//! (hub install/uninstall, model download), streaming progress back to chat.

use compact_str::CompactString;
use futures_util::StreamExt;
use socket::{ClientConfig, Connection, WalrusClient};
use std::path::PathBuf;
use teloxide::prelude::*;
use wcore::protocol::api::Client;
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
    bot: Bot,
    chat_id: i64,
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
            run_hub(&mut connection, &bot, chat_id, package, HubAction::Install).await;
        }
        BotCommand::HubUninstall { package } => {
            run_hub(
                &mut connection,
                &bot,
                chat_id,
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
                        send_text(&bot, chat_id, format!("Downloading {model}...")).await;
                    }
                    Ok(DownloadEvent::FileStart { filename, .. }) => {
                        send_text(&bot, chat_id, format!("  {filename} starting...")).await;
                    }
                    Ok(DownloadEvent::Progress { .. }) => {
                        // Too noisy for chat — skip.
                    }
                    Ok(DownloadEvent::FileEnd { filename, .. }) => {
                        send_text(&bot, chat_id, format!("  {filename} done")).await;
                    }
                    Ok(DownloadEvent::End { model }) => {
                        send_text(&bot, chat_id, format!("Download complete: {model}")).await;
                    }
                    Err(e) => {
                        tracing::warn!("download event error: {e}");
                    }
                }
            }
        }
    }
}

/// Stream a hub install/uninstall operation and send progress to chat.
async fn run_hub(
    connection: &mut Connection,
    bot: &Bot,
    chat_id: i64,
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
                    bot,
                    chat_id,
                    format!("Starting hub operation for {package}..."),
                )
                .await;
            }
            Ok(HubEvent::Step { message }) => {
                send_text(bot, chat_id, format!("  {message}")).await;
            }
            Ok(HubEvent::End { package }) => {
                send_text(bot, chat_id, format!("Done: {package}")).await;
            }
            Err(e) => {
                tracing::warn!("hub event error: {e}");
            }
        }
    }
}

/// Send a plain-text message to the chat.
async fn send_text(bot: &Bot, chat_id: i64, content: String) {
    if let Err(e) = bot.send_message(ChatId(chat_id), content).await {
        tracing::warn!("failed to send bot command reply: {e}");
    }
}
