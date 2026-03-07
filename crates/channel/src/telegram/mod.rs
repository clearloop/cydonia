//! Telegram Bot API channel adapter.
//!
//! Uses teloxide long-polling for receiving messages and `Bot::send_message`
//! for sending. Implements [`Channel`] from this crate.

pub(crate) mod command;

use crate::channel::{Channel, ChannelHandle};
use crate::message::{Attachment, AttachmentKind, ChannelMessage, Platform};
use anyhow::Result;
use compact_str::CompactString;
use futures_util::StreamExt;
use teloxide::prelude::*;
use teloxide::types::{ChatId, UpdateKind};
use teloxide::update_listeners::{AsUpdateStream, polling_default};
use tokio::sync::mpsc;

/// Telegram Bot API channel adapter.
pub struct TelegramChannel {
    /// Bot API token.
    bot_token: String,
}

impl TelegramChannel {
    /// Create a new TelegramChannel with the given bot token.
    pub fn new(bot_token: impl Into<String>) -> Self {
        Self {
            bot_token: bot_token.into(),
        }
    }
}

impl Channel for TelegramChannel {
    async fn connect(self) -> Result<ChannelHandle> {
        let bot = Bot::new(self.bot_token);
        let (tx, rx) = mpsc::unbounded_channel::<ChannelMessage>();

        // Spawn the polling task.
        let poll_bot = bot.clone();
        tokio::spawn(async move {
            poll_loop(poll_bot, tx).await;
        });

        // Build the send closure using the bot.
        let handle = ChannelHandle::new(Platform::Telegram, rx, move |msg| {
            let bot = bot.clone();
            async move {
                let chat_id: i64 = msg
                    .channel_id
                    .parse()
                    .map_err(|e| anyhow::anyhow!("invalid chat_id: {e}"))?;
                bot.send_message(ChatId(chat_id), msg.content)
                    .await
                    .map(|_| ())
                    .map_err(|e| anyhow::anyhow!(e))
            }
        });

        Ok(handle)
    }
}

/// Long-poll loop that fetches updates and sends them to the channel.
async fn poll_loop(bot: Bot, tx: mpsc::UnboundedSender<ChannelMessage>) {
    let mut listener = polling_default(bot).await;
    let stream = listener.as_stream();
    futures_util::pin_mut!(stream);

    while let Some(result) = stream.next().await {
        match result {
            Ok(update) => {
                if let Some(msg) = convert_update(update)
                    && tx.send(msg).is_err()
                {
                    tracing::info!("channel handle dropped, stopping poll loop");
                    return;
                }
            }
            Err(e) => {
                tracing::error!("telegram update error: {e}");
            }
        }
    }
}

/// Convert a teloxide `Update` to a `ChannelMessage`.
fn convert_update(update: Update) -> Option<ChannelMessage> {
    let UpdateKind::Message(msg) = update.kind else {
        return None;
    };

    let chat_id = msg.chat.id.to_string();
    let sender_id = msg
        .from
        .as_ref()
        .map(|u| u.id.to_string())
        .unwrap_or_default();
    let content = msg.text().unwrap_or("").to_owned();

    let mut attachments = Vec::new();
    if let Some(photos) = msg.photo()
        && let Some(largest) = photos.last()
    {
        attachments.push(Attachment {
            kind: AttachmentKind::Image,
            url: largest.file.id.0.clone(),
            name: None,
        });
    }
    if let Some(doc) = msg.document() {
        attachments.push(Attachment {
            kind: AttachmentKind::File,
            url: doc.file.id.0.clone(),
            name: doc.file_name.clone(),
        });
    }

    let reply_to = msg
        .reply_to_message()
        .map(|r| CompactString::from(r.id.to_string()));

    Some(ChannelMessage {
        platform: Platform::Telegram,
        channel_id: CompactString::from(chat_id),
        sender_id: CompactString::from(sender_id),
        content,
        attachments,
        reply_to,
        timestamp: msg.date.timestamp() as u64,
    })
}
