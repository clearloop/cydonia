//! Channel router — connects platform channels and routes messages to agents.
//!
//! Owns the lifecycle of all channel connections: building the routing table,
//! connecting to platforms (Telegram, etc.), and running message loops. The
//! daemon passes a callback for agent dispatch, keeping this crate decoupled
//! from the runtime.

pub use channel::{
    ChannelMessage, ChannelRouter, ChannelSender, Platform, RoutingRule, parse_platform,
};
use compact_str::CompactString;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Channel configuration entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelConfig {
    /// Platform name (e.g. "telegram").
    pub platform: CompactString,
    /// Bot token for the platform.
    pub bot_token: String,
    /// Default agent for this channel.
    pub agent: CompactString,
    /// Optional specific channel ID for exact routing.
    pub channel_id: Option<CompactString>,
}

/// Build a [`ChannelRouter`] from channel config entries.
pub fn build_router(configs: &[ChannelConfig]) -> ChannelRouter {
    let mut rules = Vec::new();
    let mut default_agent = None;

    for ch in configs {
        let Ok(platform) = parse_platform(&ch.platform) else {
            tracing::warn!("unknown platform '{}', skipping", ch.platform);
            continue;
        };
        rules.push(RoutingRule {
            platform,
            channel_id: ch.channel_id.clone(),
            agent: ch.agent.clone(),
        });
        if default_agent.is_none() {
            default_agent = Some(ch.agent.clone());
        }
    }

    ChannelRouter::new(rules, default_agent)
}

/// Connect all configured channels and spawn message loops.
///
/// For each channel config, connects to the platform and spawns a task that:
/// 1. Receives messages from the platform
/// 2. Routes to the correct agent via the router
/// 3. Calls `on_message(agent, content)` to get a reply
/// 4. Sends the reply back through the channel
///
/// `on_message` is a callback that decouples from the daemon's Runtime type.
pub async fn spawn_channels<F, Fut>(
    configs: &[ChannelConfig],
    router: Arc<ChannelRouter>,
    on_message: Arc<F>,
) where
    F: Fn(CompactString, String) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<String, String>> + Send + 'static,
{
    for ch in configs {
        let Ok(platform) = parse_platform(&ch.platform) else {
            continue;
        };

        match platform {
            Platform::Telegram => {
                let tg = telegram::TelegramChannel::new(ch.bot_token.clone());
                match channel::Channel::connect(tg).await {
                    Ok(mut handle) => {
                        let (tx, rx) = mpsc::unbounded_channel();
                        let sender = handle.sender();
                        let rr = Arc::clone(&router);
                        let cb = Arc::clone(&on_message);

                        tokio::spawn(async move {
                            while let Some(msg) = handle.recv().await {
                                if tx.send(msg).is_err() {
                                    break;
                                }
                            }
                        });

                        tokio::spawn(channel_loop(rx, sender, rr, cb));

                        tracing::info!(platform = "telegram", "channel transport started");
                    }
                    Err(e) => {
                        tracing::error!(platform = "telegram", "failed to connect channel: {e}");
                    }
                }
            }
            _ => {
                tracing::warn!(platform = %ch.platform, "unsupported channel platform");
            }
        }
    }
}

/// Message loop for a single channel connection.
///
/// Receives messages, routes to agents, dispatches via callback, sends replies.
async fn channel_loop<F, Fut>(
    mut rx: mpsc::UnboundedReceiver<ChannelMessage>,
    sender: ChannelSender,
    router: Arc<ChannelRouter>,
    on_message: Arc<F>,
) where
    F: Fn(CompactString, String) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<String, String>> + Send + 'static,
{
    while let Some(msg) = rx.recv().await {
        let platform = msg.platform;
        let channel_id = msg.channel_id.clone();
        let sender_id = msg.sender_id.clone();

        let Some(agent) = router.route(platform, &channel_id) else {
            tracing::warn!(
                ?platform,
                %channel_id,
                "no agent route found, dropping message"
            );
            continue;
        };

        let agent = agent.clone();
        let content = msg.content.clone();

        tracing::info!(%agent, %channel_id, %sender_id, "channel dispatch");

        match on_message(agent.clone(), content).await {
            Ok(reply) => {
                let reply_msg = ChannelMessage {
                    platform,
                    channel_id,
                    sender_id: Default::default(),
                    content: reply,
                    attachments: Vec::new(),
                    reply_to: Some(sender_id),
                    timestamp: 0,
                };
                if let Err(e) = sender.send(reply_msg).await {
                    tracing::warn!(%agent, "failed to send channel reply: {e}");
                }
            }
            Err(e) => {
                tracing::warn!(%agent, "dispatch error: {e}");
            }
        }
    }

    tracing::info!(platform = ?sender.platform(), "channel loop ended");
}
