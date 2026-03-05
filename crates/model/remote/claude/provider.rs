//! Model trait implementation for the Claude (Anthropic) provider.

use super::{Claude, Request};
use anyhow::Result;
use compact_str::CompactString;
use futures_core::Stream;
use wcore::model::{
    Choice, CompletionMeta, CompletionTokensDetails, Delta, FinishReason, Model, Response,
    StreamChunk, Usage,
};

/// Raw Anthropic non-streaming response.
#[derive(serde::Deserialize)]
struct AnthropicResponse {
    id: CompactString,
    model: CompactString,
    content: Vec<ContentBlock>,
    stop_reason: Option<CompactString>,
    usage: AnthropicUsage,
}

#[derive(serde::Deserialize)]
#[serde(tag = "type")]
enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: CompactString,
        name: CompactString,
        input: serde_json::Value,
    },
}

#[derive(serde::Deserialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
}

impl Model for Claude {
    async fn send(&self, request: &wcore::model::Request) -> Result<Response> {
        let body = Request::from(request.clone());
        let text = self.http.send_raw(&body).await?;
        tracing::trace!("response: {text}");
        let raw: AnthropicResponse = serde_json::from_str(&text)?;
        Ok(to_response(raw))
    }

    fn stream(
        &self,
        request: wcore::model::Request,
    ) -> impl Stream<Item = Result<StreamChunk>> + Send {
        let body = serde_json::to_value(Request::from(request).stream())
            .expect("claude request serialization failed");
        self.http.stream_anthropic(body)
    }

    fn active_model(&self) -> CompactString {
        self.model.clone()
    }
}

/// Convert an Anthropic response to the unified `Response` format.
fn to_response(raw: AnthropicResponse) -> Response {
    let mut content = String::new();
    let mut tool_calls = Vec::new();

    for block in raw.content {
        match block {
            ContentBlock::Text { text } => {
                if !content.is_empty() {
                    content.push('\n');
                }
                content.push_str(&text);
            }
            ContentBlock::ToolUse { id, name, input } => {
                tool_calls.push(wcore::model::ToolCall {
                    id,
                    index: tool_calls.len() as u32,
                    call_type: "function".into(),
                    function: wcore::model::FunctionCall {
                        name,
                        arguments: serde_json::to_string(&input).unwrap_or_default(),
                    },
                });
            }
        }
    }

    let finish_reason = raw.stop_reason.as_deref().map(|r| match r {
        "end_turn" | "stop" => FinishReason::Stop,
        "max_tokens" => FinishReason::Length,
        "tool_use" => FinishReason::ToolCalls,
        _ => FinishReason::Stop,
    });

    Response {
        meta: CompletionMeta {
            id: raw.id,
            object: "chat.completion".into(),
            model: raw.model,
            ..Default::default()
        },
        choices: vec![Choice {
            index: 0,
            delta: Delta {
                role: Some(wcore::model::Role::Assistant),
                content: Some(content),
                reasoning_content: None,
                tool_calls: if tool_calls.is_empty() {
                    None
                } else {
                    Some(tool_calls)
                },
            },
            finish_reason,
            logprobs: None,
        }],
        usage: Usage {
            prompt_tokens: raw.usage.input_tokens,
            completion_tokens: raw.usage.output_tokens,
            total_tokens: raw.usage.input_tokens + raw.usage.output_tokens,
            prompt_cache_hit_tokens: None,
            prompt_cache_miss_tokens: None,
            completion_tokens_details: Some(CompletionTokensDetails {
                reasoning_tokens: None,
            }),
        },
    }
}
