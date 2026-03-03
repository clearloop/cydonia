//! Channel message loop — receives from a ChannelHandle, dispatches to
//! agents via the shared dispatch path, and sends replies back.

use crate::gateway::GatewayHook;
use crate::gateway::dispatch::{AgentLock, dispatch_send};
use channel::{ChannelMessage, ChannelRouter, ChannelSender};
use model::ProviderManager;
use runtime::Runtime;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{info, warn};

/// Run the channel receive loop until the channel closes or shutdown fires.
///
/// For each incoming message, routes to the correct agent via the router,
/// dispatches through the shared agent lock, and sends the reply back.
pub async fn channel_loop(
    mut rx: mpsc::UnboundedReceiver<ChannelMessage>,
    sender: ChannelSender,
    runtime: Arc<Runtime<ProviderManager, GatewayHook>>,
    locks: Arc<AgentLock>,
    router: Arc<ChannelRouter>,
) {
    while let Some(msg) = rx.recv().await {
        let platform = msg.platform;
        let channel_id = msg.channel_id.clone();
        let sender_id = msg.sender_id.clone();

        let Some(agent) = router.route(platform, &channel_id) else {
            warn!(
                ?platform,
                %channel_id,
                "no agent route found, dropping message"
            );
            continue;
        };

        let agent = agent.clone();
        let content = msg.content.clone();

        info!(%agent, %channel_id, %sender_id, "channel dispatch");

        match dispatch_send(&runtime, &locks, &agent, &content).await {
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
                    warn!(%agent, "failed to send channel reply: {e}");
                }
            }
            Err(e) => {
                warn!(%agent, "dispatch error: {e}");
            }
        }
    }

    info!(platform = ?sender.platform(), "channel loop ended");
}
