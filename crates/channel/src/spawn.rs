//! Channel configuration and spawn logic.
//!
//! Connects the Telegram bot and routes messages to agents via the daemon
//! event loop. Bot commands (`/hub`, `/model`) are dispatched directly to
//! walrusd over the Unix domain socket.

use crate::message::ChannelMessage;
use crate::telegram::command::{dispatch_command, parse_command};
use crate::telegram::poll_loop;
use compact_str::CompactString;
use serde::{Deserialize, Serialize};
use std::{future::Future, path::PathBuf, sync::Arc};
use teloxide::prelude::*;
use tokio::sync::mpsc;

/// Top-level channel configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelConfig {
    /// Telegram bot configuration.
    pub telegram: Option<TelegramConfig>,
}

/// Configuration for the Telegram bot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramConfig {
    /// Bot API token.
    pub bot: String,
    /// Agent to route messages to. Falls back to `default_agent` if absent.
    pub agent: Option<String>,
}

/// Connect configured channels and spawn message loops.
///
/// If `config.telegram` is `None`, this is a no-op.
/// `default_agent` is used when `TelegramConfig::agent` is not set.
/// `socket_path` enables bot command dispatch; if `None`, `/` commands are dropped.
pub async fn spawn_channels<F, Fut>(
    config: &ChannelConfig,
    default_agent: CompactString,
    on_message: Arc<F>,
    socket_path: Option<PathBuf>,
) where
    F: Fn(CompactString, String) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<String, String>> + Send + 'static,
{
    let Some(tg) = &config.telegram else {
        return;
    };

    let agent = tg
        .agent
        .as_deref()
        .map(CompactString::from)
        .unwrap_or(default_agent);

    let bot = Bot::new(&tg.bot);
    let (tx, rx) = mpsc::unbounded_channel::<ChannelMessage>();

    let poll_bot = bot.clone();
    tokio::spawn(async move {
        poll_loop(poll_bot, tx).await;
    });

    let socket_path = Arc::new(socket_path);
    tokio::spawn(channel_loop(rx, bot, agent, on_message, socket_path));

    tracing::info!(platform = "telegram", "channel transport started");
}

/// Message loop: routes incoming messages to agents or bot commands.
async fn channel_loop<F, Fut>(
    mut rx: mpsc::UnboundedReceiver<ChannelMessage>,
    bot: Bot,
    agent: CompactString,
    on_message: Arc<F>,
    socket_path: Arc<Option<PathBuf>>,
) where
    F: Fn(CompactString, String) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<String, String>> + Send + 'static,
{
    while let Some(msg) = rx.recv().await {
        let chat_id = msg.chat_id;
        let content = msg.content.clone();

        tracing::info!(%agent, chat_id, "channel dispatch");

        // Bot command path.
        if content.starts_with('/') {
            match parse_command(&content) {
                Some(cmd) => {
                    if let Some(sp) = socket_path.as_ref().as_ref() {
                        tokio::spawn(dispatch_command(cmd, sp.clone(), bot.clone(), chat_id));
                    } else {
                        tracing::warn!(chat_id, "bot command ignored: no socket_path configured");
                    }
                }
                None => {
                    tracing::warn!(chat_id, content, "unrecognised bot command");
                    let hint = "Unknown command. Available: /hub install <pkg>, /hub uninstall <pkg>, /model download <model>";
                    if let Err(e) = bot.send_message(ChatId(chat_id), hint).await {
                        tracing::warn!("failed to send command hint: {e}");
                    }
                }
            }
            continue;
        }

        // Normal agent chat path.
        match on_message(agent.clone(), content).await {
            Ok(reply) => {
                if let Err(e) = bot.send_message(ChatId(chat_id), reply).await {
                    tracing::warn!(%agent, "failed to send channel reply: {e}");
                }
            }
            Err(e) => {
                tracing::warn!(%agent, "dispatch error: {e}");
            }
        }
    }

    tracing::info!(platform = "telegram", "channel loop ended");
}
