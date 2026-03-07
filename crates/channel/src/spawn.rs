//! Channel configuration and spawn logic.
//!
//! Connects configured platform bots (Telegram, Discord) and routes messages
//! to agents via the daemon event loop. Bot commands (`/hub`, `/model`) are
//! dispatched directly to walrusd over the Unix domain socket.

use crate::command::parse_command;
use crate::message::ChannelMessage;
use crate::telegram::command::dispatch_command as tg_dispatch_command;
use crate::telegram::poll_loop;
use compact_str::CompactString;
use serde::{Deserialize, Serialize};
use serenity::model::id::ChannelId;
use std::{future::Future, path::PathBuf, sync::Arc};
use teloxide::prelude::*;
use tokio::sync::mpsc;

/// Top-level channel configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelConfig {
    /// Telegram bot configuration.
    pub telegram: Option<TelegramConfig>,
    /// Discord bot configuration.
    pub discord: Option<DiscordConfig>,
}

/// Configuration for the Telegram bot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramConfig {
    /// Bot API token.
    pub bot: String,
    /// Agent to route messages to. Falls back to `default_agent` if absent.
    pub agent: Option<String>,
}

/// Configuration for the Discord bot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordConfig {
    /// Bot token.
    pub token: String,
    /// Agent to route messages to. Falls back to `default_agent` if absent.
    pub agent: Option<String>,
}

/// Connect configured channels and spawn message loops.
///
/// Spawns transports for each configured platform (Telegram, Discord).
/// `default_agent` is used when a platform config does not specify an agent.
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
    let socket_path = Arc::new(socket_path);

    // Telegram transport.
    if let Some(tg) = &config.telegram {
        let agent = tg
            .agent
            .as_deref()
            .map(CompactString::from)
            .unwrap_or_else(|| default_agent.clone());

        let bot = Bot::new(&tg.bot);
        let (tx, rx) = mpsc::unbounded_channel::<ChannelMessage>();

        let poll_bot = bot.clone();
        tokio::spawn(async move {
            poll_loop(poll_bot, tx).await;
        });

        tokio::spawn(telegram_loop(
            rx,
            bot,
            agent,
            on_message.clone(),
            socket_path.clone(),
        ));

        tracing::info!(platform = "telegram", "channel transport started");
    }

    // Discord transport.
    if let Some(dc) = &config.discord {
        let agent = dc
            .agent
            .as_deref()
            .map(CompactString::from)
            .unwrap_or_else(|| default_agent.clone());

        let (msg_tx, msg_rx) = mpsc::unbounded_channel::<ChannelMessage>();
        let (http_tx, http_rx) = tokio::sync::oneshot::channel();

        let token = dc.token.clone();
        tokio::spawn(async move {
            crate::discord::event_loop(&token, msg_tx, http_tx).await;
        });

        let on_msg = on_message.clone();
        let sp = socket_path.clone();
        tokio::spawn(async move {
            match http_rx.await {
                Ok(http) => {
                    discord_loop(msg_rx, http, agent, on_msg, sp).await;
                }
                Err(_) => {
                    tracing::error!("discord gateway failed to send http client");
                }
            }
        });

        tracing::info!(platform = "discord", "channel transport started");
    }
}

/// Telegram message loop: routes incoming messages to agents or bot commands.
async fn telegram_loop<F, Fut>(
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

        tracing::info!(%agent, chat_id, "telegram dispatch");

        // Bot command path.
        if content.starts_with('/') {
            match parse_command(&content) {
                Some(cmd) => {
                    if let Some(sp) = socket_path.as_ref().as_ref() {
                        tokio::spawn(tg_dispatch_command(cmd, sp.clone(), bot.clone(), chat_id));
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

/// Discord message loop: routes incoming messages to agents or bot commands.
async fn discord_loop<F, Fut>(
    mut rx: mpsc::UnboundedReceiver<ChannelMessage>,
    http: Arc<serenity::http::Http>,
    agent: CompactString,
    on_message: Arc<F>,
    socket_path: Arc<Option<PathBuf>>,
) where
    F: Fn(CompactString, String) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<String, String>> + Send + 'static,
{
    while let Some(msg) = rx.recv().await {
        let chat_id = msg.chat_id;
        let channel_id = ChannelId::new(chat_id as u64);
        let content = msg.content.clone();

        tracing::info!(%agent, chat_id, "discord dispatch");

        // Bot command path.
        if content.starts_with('/') {
            match parse_command(&content) {
                Some(cmd) => {
                    if let Some(sp) = socket_path.as_ref().as_ref() {
                        tokio::spawn(crate::discord::command::dispatch_command(
                            cmd,
                            sp.clone(),
                            http.clone(),
                            channel_id,
                        ));
                    } else {
                        tracing::warn!(chat_id, "bot command ignored: no socket_path configured");
                    }
                }
                None => {
                    tracing::warn!(chat_id, content, "unrecognised bot command");
                    let hint = "Unknown command. Available: /hub install <pkg>, /hub uninstall <pkg>, /model download <model>";
                    crate::discord::send_text(&http, channel_id, hint.to_owned()).await;
                }
            }
            continue;
        }

        // Normal agent chat path.
        match on_message(agent.clone(), content).await {
            Ok(reply) => {
                crate::discord::send_text(&http, channel_id, reply).await;
            }
            Err(e) => {
                tracing::warn!(%agent, "dispatch error: {e}");
            }
        }
    }

    tracing::info!(platform = "discord", "channel loop ended");
}
