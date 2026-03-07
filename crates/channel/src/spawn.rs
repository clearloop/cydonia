//! Channel spawn logic.
//!
//! Connects configured platform bots (Telegram, Discord) and routes messages
//! to agents via callbacks. Bot commands (`/hub`, `/model`) are dispatched
//! through the `on_command` callback which streams `ServerMessage` results.

use crate::command::parse_command;
use crate::config::{ChannelConfig, DiscordConfig, TelegramConfig};
use crate::message::ChannelMessage;
use compact_str::CompactString;
use serenity::model::id::ChannelId;
use std::{future::Future, sync::Arc};
use teloxide::prelude::*;
use tokio::sync::mpsc;
use wcore::protocol::message::{client::ClientMessage, server::ServerMessage};

/// Connect configured channels and spawn message loops.
///
/// Spawns transports for each configured platform (Telegram, Discord).
/// `default_agent` is used when a platform config does not specify an agent.
/// `on_command` dispatches bot commands and returns a receiver for streamed results.
pub async fn spawn_channels<F, Fut, C, CFut>(
    config: &ChannelConfig,
    default_agent: CompactString,
    on_message: Arc<F>,
    on_command: Arc<C>,
) where
    F: Fn(CompactString, String) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<String, String>> + Send + 'static,
    C: Fn(ClientMessage) -> CFut + Send + Sync + 'static,
    CFut: Future<Output = mpsc::UnboundedReceiver<ServerMessage>> + Send + 'static,
{
    // Telegram transport.
    if let Some(tg) = &config.telegram {
        spawn_telegram(tg, &default_agent, on_message.clone(), on_command.clone()).await;
    }

    // Discord transport.
    if let Some(dc) = &config.discord {
        spawn_discord(dc, &default_agent, on_message.clone(), on_command.clone()).await;
    }
}

async fn spawn_telegram<F, Fut, C, CFut>(
    tg: &TelegramConfig,
    default_agent: &CompactString,
    on_message: Arc<F>,
    on_command: Arc<C>,
) where
    F: Fn(CompactString, String) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<String, String>> + Send + 'static,
    C: Fn(ClientMessage) -> CFut + Send + Sync + 'static,
    CFut: Future<Output = mpsc::UnboundedReceiver<ServerMessage>> + Send + 'static,
{
    let agent = tg
        .agent
        .as_deref()
        .map(CompactString::from)
        .unwrap_or_else(|| default_agent.clone());

    let bot = Bot::new(&tg.bot);
    let (tx, rx) = mpsc::unbounded_channel::<ChannelMessage>();

    let poll_bot = bot.clone();
    tokio::spawn(async move {
        crate::telegram::poll_loop(poll_bot, tx).await;
    });

    tokio::spawn(telegram_loop(rx, bot, agent, on_message, on_command));
    tracing::info!(platform = "telegram", "channel transport started");
}

async fn spawn_discord<F, Fut, C, CFut>(
    dc: &DiscordConfig,
    default_agent: &CompactString,
    on_message: Arc<F>,
    on_command: Arc<C>,
) where
    F: Fn(CompactString, String) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<String, String>> + Send + 'static,
    C: Fn(ClientMessage) -> CFut + Send + Sync + 'static,
    CFut: Future<Output = mpsc::UnboundedReceiver<ServerMessage>> + Send + 'static,
{
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

    tokio::spawn(async move {
        match http_rx.await {
            Ok(http) => {
                discord_loop(msg_rx, http, agent, on_message, on_command).await;
            }
            Err(_) => {
                tracing::error!("discord gateway failed to send http client");
            }
        }
    });

    tracing::info!(platform = "discord", "channel transport started");
}

/// Telegram message loop: routes incoming messages to agents or bot commands.
async fn telegram_loop<F, Fut, C, CFut>(
    mut rx: mpsc::UnboundedReceiver<ChannelMessage>,
    bot: Bot,
    agent: CompactString,
    on_message: Arc<F>,
    on_command: Arc<C>,
) where
    F: Fn(CompactString, String) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<String, String>> + Send + 'static,
    C: Fn(ClientMessage) -> CFut + Send + Sync + 'static,
    CFut: Future<Output = mpsc::UnboundedReceiver<ServerMessage>> + Send + 'static,
{
    while let Some(msg) = rx.recv().await {
        let chat_id = msg.chat_id;
        let content = msg.content.clone();

        tracing::info!(%agent, chat_id, "telegram dispatch");

        // Bot command path.
        if content.starts_with('/') {
            match parse_command(&content) {
                Some(cmd) => {
                    let b = bot.clone();
                    let oc = on_command.clone();
                    tokio::spawn(async move {
                        crate::telegram::command::dispatch_command(cmd, oc, b, chat_id).await;
                    });
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
async fn discord_loop<F, Fut, C, CFut>(
    mut rx: mpsc::UnboundedReceiver<ChannelMessage>,
    http: Arc<serenity::http::Http>,
    agent: CompactString,
    on_message: Arc<F>,
    on_command: Arc<C>,
) where
    F: Fn(CompactString, String) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<String, String>> + Send + 'static,
    C: Fn(ClientMessage) -> CFut + Send + Sync + 'static,
    CFut: Future<Output = mpsc::UnboundedReceiver<ServerMessage>> + Send + 'static,
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
                    let h = http.clone();
                    let oc = on_command.clone();
                    tokio::spawn(async move {
                        crate::discord::command::dispatch_command(cmd, oc, h, channel_id).await;
                    });
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
