//! SSE event parsing for the Anthropic streaming Messages API.
//!
//! Anthropic streaming events differ from OpenAI's format:
//! - `message_start` — initial message metadata
//! - `content_block_start` — begin a content block (text or tool_use)
//! - `content_block_delta` — incremental content (text_delta or input_json_delta)
//! - `content_block_stop` — end of a content block
//! - `message_delta` — final stop_reason and usage
//! - `message_stop` — end of message
//!
//! `parse_sse_block` is `pub(crate)` so `HttpProvider::stream_anthropic` can call it.

use compact_str::CompactString;
use serde::Deserialize;
use wcore::model::{
    Choice, CompletionMeta, CompletionTokensDetails, Delta, FinishReason, FunctionCall,
    StreamChunk, ToolCall, Usage,
};

/// A raw SSE event from the Anthropic streaming API.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum Event {
    /// Initial message metadata.
    #[serde(rename = "message_start")]
    MessageStart { message: MessageMeta },
    /// Begin a content block.
    #[serde(rename = "content_block_start")]
    ContentBlockStart {
        index: u32,
        content_block: ContentBlock,
    },
    /// Incremental content within a block.
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta { index: u32, delta: BlockDelta },
    /// End of a content block.
    #[serde(rename = "content_block_stop")]
    ContentBlockStop {},
    /// Final message delta (stop reason + usage).
    #[serde(rename = "message_delta")]
    MessageDelta {
        delta: MessageDeltaBody,
        usage: MessageDeltaUsage,
    },
    /// End of message.
    #[serde(rename = "message_stop")]
    MessageStop,
    /// Ping (keep-alive).
    #[serde(rename = "ping")]
    Ping,
    /// Catch-all for unknown event types.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
pub struct MessageMeta {
    pub id: CompactString,
    pub model: CompactString,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: CompactString,
        name: CompactString,
    },
    #[serde(rename = "thinking")]
    Thinking { thinking: String },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
#[allow(clippy::enum_variant_names)]
pub enum BlockDelta {
    #[serde(rename = "text_delta")]
    TextDelta { text: String },
    #[serde(rename = "input_json_delta")]
    InputJsonDelta { partial_json: String },
    #[serde(rename = "thinking_delta")]
    ThinkingDelta { thinking: String },
}

#[derive(Debug, Deserialize)]
pub struct MessageDeltaBody {
    pub stop_reason: Option<CompactString>,
}

#[derive(Debug, Deserialize)]
pub struct MessageDeltaUsage {
    pub output_tokens: u32,
}

impl Event {
    /// Convert this Anthropic event to a walrus `StreamChunk`.
    /// Returns `None` for events that don't produce output (ping, stop, unknown).
    pub fn into_chunk(self) -> Option<StreamChunk> {
        match self {
            Self::MessageStart { message } => Some(StreamChunk {
                meta: CompletionMeta {
                    id: message.id,
                    object: "chat.completion.chunk".into(),
                    model: message.model,
                    ..Default::default()
                },
                ..Default::default()
            }),
            Self::ContentBlockStart {
                content_block: ContentBlock::Text { text },
                ..
            } => {
                if text.is_empty() {
                    None
                } else {
                    Some(StreamChunk {
                        choices: vec![Choice {
                            delta: Delta {
                                content: Some(text),
                                ..Default::default()
                            },
                            ..Default::default()
                        }],
                        ..Default::default()
                    })
                }
            }
            Self::ContentBlockStart {
                content_block: ContentBlock::Thinking { thinking },
                ..
            } => {
                if thinking.is_empty() {
                    None
                } else {
                    Some(StreamChunk {
                        choices: vec![Choice {
                            delta: Delta {
                                reasoning_content: Some(thinking),
                                ..Default::default()
                            },
                            ..Default::default()
                        }],
                        ..Default::default()
                    })
                }
            }
            Self::ContentBlockStart {
                index,
                content_block: ContentBlock::ToolUse { id, name },
            } => Some(StreamChunk {
                choices: vec![Choice {
                    delta: Delta {
                        tool_calls: Some(vec![ToolCall {
                            id,
                            index,
                            call_type: "function".into(),
                            function: FunctionCall {
                                name,
                                arguments: String::new(),
                            },
                        }]),
                        ..Default::default()
                    },
                    ..Default::default()
                }],
                ..Default::default()
            }),
            Self::ContentBlockDelta {
                delta: BlockDelta::TextDelta { text },
                ..
            } => Some(StreamChunk {
                choices: vec![Choice {
                    delta: Delta {
                        content: Some(text),
                        ..Default::default()
                    },
                    ..Default::default()
                }],
                ..Default::default()
            }),
            Self::ContentBlockDelta {
                delta: BlockDelta::ThinkingDelta { thinking },
                ..
            } => Some(StreamChunk {
                choices: vec![Choice {
                    delta: Delta {
                        reasoning_content: Some(thinking),
                        ..Default::default()
                    },
                    ..Default::default()
                }],
                ..Default::default()
            }),
            Self::ContentBlockDelta {
                index,
                delta: BlockDelta::InputJsonDelta { partial_json },
            } => Some(StreamChunk {
                choices: vec![Choice {
                    delta: Delta {
                        tool_calls: Some(vec![ToolCall {
                            index,
                            function: FunctionCall {
                                arguments: partial_json,
                                ..Default::default()
                            },
                            ..Default::default()
                        }]),
                        ..Default::default()
                    },
                    ..Default::default()
                }],
                ..Default::default()
            }),
            Self::MessageDelta { delta, usage } => {
                let reason = delta.stop_reason.as_deref().map(|r| match r {
                    "end_turn" | "stop" => FinishReason::Stop,
                    "max_tokens" => FinishReason::Length,
                    "tool_use" => FinishReason::ToolCalls,
                    _ => FinishReason::Stop,
                });
                Some(StreamChunk {
                    choices: vec![Choice {
                        finish_reason: reason,
                        ..Default::default()
                    }],
                    usage: Some(Usage {
                        prompt_tokens: 0,
                        completion_tokens: usage.output_tokens,
                        total_tokens: usage.output_tokens,
                        prompt_cache_hit_tokens: None,
                        prompt_cache_miss_tokens: None,
                        completion_tokens_details: Some(CompletionTokensDetails {
                            reasoning_tokens: None,
                        }),
                    }),
                    ..Default::default()
                })
            }
            Self::ContentBlockStop {} | Self::MessageStop | Self::Ping | Self::Unknown => None,
        }
    }
}

/// Parse a single Anthropic SSE block (may contain `event:` and `data:` lines).
///
/// Returns the corresponding `StreamChunk` or `None` for terminal/no-op events.
pub(crate) fn parse_sse_block(block: &str) -> Option<StreamChunk> {
    let mut data_str = None;
    for line in block.lines() {
        if let Some(d) = line.strip_prefix("data: ") {
            data_str = Some(d.trim());
        }
    }
    let data = data_str?;
    if data == "[DONE]" {
        return None;
    }
    match serde_json::from_str::<Event>(data) {
        Ok(event) => event.into_chunk(),
        Err(e) => {
            tracing::warn!("failed to parse anthropic event: {e}, data: {data}");
            None
        }
    }
}
