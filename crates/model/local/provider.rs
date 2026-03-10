//! Model trait implementation for the Local provider.

use crate::local::Local;
use anyhow::Result;
use async_stream::try_stream;
use compact_str::CompactString;
use futures_core::Stream;
use std::collections::HashMap;
use wcore::model::{
    Choice, CompletionMeta, Delta, FunctionCall, Model, Response, Role, StreamChunk, ToolCall,
    Usage,
};

impl Model for Local {
    async fn send(&self, request: &wcore::model::Request) -> Result<Response> {
        let model = self.ready_model()?;
        let mr_request = build_request(request);
        let resp = model.send_chat_request(mr_request).await?;
        Ok(to_response(resp))
    }

    fn stream(
        &self,
        request: wcore::model::Request,
    ) -> impl Stream<Item = Result<StreamChunk>> + Send {
        let model_result = self.ready_model();
        try_stream! {
            let model = model_result?;
            let mr_request = build_request(&request);
            let mut stream = model.stream_chat_request(mr_request).await?;
            let mut filter = ToolCallFilter::new();
            while let Some(resp) = stream.next().await {
                match resp {
                    mistralrs::Response::Chunk(chunk) => {
                        for sc in filter.accept(chunk) {
                            yield sc;
                        }
                    }
                    mistralrs::Response::Done(done) => {
                        for sc in filter.finish(done) {
                            yield sc;
                        }
                        break;
                    }
                    mistralrs::Response::InternalError(e)
                    | mistralrs::Response::ValidationError(e) => {
                        Err(anyhow::anyhow!("{e}"))?;
                    }
                    mistralrs::Response::ModelError(msg, _) => {
                        Err(anyhow::anyhow!("model error: {msg}"))?;
                    }
                    _ => {}
                }
            }
        }
    }

    fn context_limit(&self, model: &str) -> usize {
        self.context_length(model)
            .unwrap_or_else(|| wcore::model::default_context_limit(model))
    }

    fn active_model(&self) -> CompactString {
        self.model_id.clone()
    }
}

/// Known tool call tag prefixes emitted by local models as raw text.
///
/// During streaming, mistralrs cannot parse incomplete tags, so they leak
/// into the content field. We detect these prefixes and suppress content
/// once a tool call tag is detected, relying on the final `Response::Done`
/// for structured tool calls.
const TOOL_CALL_PREFIXES: &[&str] = &[
    "<tool_call>",
    "<｜tool\u{2581}call\u{2581}begin｜>",
    "[TOOL_CALLS]",
    "<|python_tag|>",
];

/// Filters tool call tags from streaming content chunks.
///
/// Local LLMs (Qwen, DeepSeek, etc.) emit tool calls as XML-like tags in
/// text content during streaming. mistralrs parses them in the final
/// `Response::Done` but not in incremental chunks. This filter suppresses
/// raw tag text and emits a final `StreamChunk` with structured tool calls
/// from the Done response.
struct ToolCallFilter {
    /// Accumulated content text for prefix detection.
    buffer: String,
    /// Whether we've detected a tool call prefix and are suppressing content.
    suppressing: bool,
}

impl ToolCallFilter {
    fn new() -> Self {
        Self {
            buffer: String::new(),
            suppressing: false,
        }
    }

    /// Flush buffered content as a content-only `StreamChunk`, clearing the
    /// buffer. Returns `None` if the buffer is empty.
    fn flush(&mut self) -> Option<StreamChunk> {
        if self.buffer.is_empty() {
            return None;
        }
        let content = std::mem::take(&mut self.buffer);
        Some(StreamChunk::text(content))
    }

    /// Process a streaming chunk. Returns chunks to yield (0, 1, or 2).
    fn accept(&mut self, chunk: mistralrs::ChatCompletionChunkResponse) -> Vec<StreamChunk> {
        let mut out = Vec::new();

        // If already suppressing, drop all chunks.
        if self.suppressing {
            return out;
        }

        // Check if this chunk has text content.
        let has_content = chunk
            .choices
            .first()
            .and_then(|c| c.delta.content.as_deref())
            .is_some_and(|s| !s.is_empty());

        if has_content {
            let text = chunk.choices[0].delta.content.as_deref().unwrap();
            self.buffer.push_str(text);
            tracing::trace!(buffer = %self.buffer, "tool_call_filter: buffered content");

            // Full match — enter suppression, discard buffered tag text.
            if TOOL_CALL_PREFIXES.iter().any(|p| self.buffer.contains(p)) {
                tracing::debug!(buffer = %self.buffer, "tool_call_filter: suppressing tool call tags");
                self.suppressing = true;
                self.buffer.clear();
                return out;
            }

            // Partial match — hold the buffer, don't yield yet.
            // Only check when the buffer has at least 2 chars — no single
            // character uniquely identifies a tool call prefix start, so
            // short buffers flush immediately for responsive streaming.
            if self.buffer.len() >= 2
                && TOOL_CALL_PREFIXES
                    .iter()
                    .any(|p| is_partial_prefix(&self.buffer, p))
            {
                tracing::trace!(buffer = %self.buffer, "tool_call_filter: partial prefix match, holding");
                return out;
            }

            // No match possible — flush the buffer.
            if let Some(sc) = self.flush() {
                out.push(sc);
            }
        } else {
            // Non-content chunk (reasoning, finish_reason, structured tool_calls).
            // Flush any buffered text first, then pass through.
            if let Some(sc) = self.flush() {
                out.push(sc);
            }
            out.push(to_stream_chunk(chunk));
        }

        out
    }

    /// Process the final `Response::Done`. Returns chunks to yield — either
    /// structured tool calls from the Done response, or remaining buffered
    /// content if no tool calls were found.
    fn finish(mut self, done: mistralrs::ChatCompletionResponse) -> Vec<StreamChunk> {
        let mut out = Vec::new();
        let has_tool_calls = done
            .choices
            .first()
            .and_then(|c| c.message.tool_calls.as_ref())
            .is_some_and(|tc| !tc.is_empty());
        tracing::debug!(
            suppressing = self.suppressing,
            has_tool_calls,
            buffer_len = self.buffer.len(),
            "tool_call_filter: finish"
        );

        // If we were suppressing, emit tool calls from Done.
        if self.suppressing {
            if let Some(tc_chunk) = done
                .choices
                .first()
                .and_then(|c| c.message.tool_calls.as_ref())
                .filter(|tc| !tc.is_empty())
                .map(|tcs| {
                    let calls: Vec<ToolCall> = tcs.iter().cloned().map(convert_tool_call).collect();
                    StreamChunk::tool(&calls)
                })
            {
                out.push(tc_chunk);
            }
            return out;
        }

        // Not suppressing — flush any remaining buffered text.
        if let Some(sc) = self.flush() {
            out.push(sc);
        }
        out
    }
}

/// Check if `text` ends with a string that is a prefix of `pattern`.
///
/// For example, `text="Hello <tool"` is a partial prefix of `"<tool_call>"`.
/// Iterates over char boundaries to avoid panics on multibyte patterns.
fn is_partial_prefix(text: &str, pattern: &str) -> bool {
    pattern
        .char_indices()
        .skip(1)
        .any(|(i, _)| text.ends_with(&pattern[..i]))
}

/// Build a mistralrs `RequestBuilder` from a walrus `Request`.
fn build_request(request: &wcore::model::Request) -> mistralrs::RequestBuilder {
    let mut builder = mistralrs::RequestBuilder::new();

    for msg in &request.messages {
        match msg.role {
            Role::System => {
                builder = builder.add_message(mistralrs::TextMessageRole::System, &msg.content);
            }
            Role::User => {
                builder = builder.add_message(mistralrs::TextMessageRole::User, &msg.content);
            }
            Role::Assistant => {
                if msg.tool_calls.is_empty() {
                    builder =
                        builder.add_message(mistralrs::TextMessageRole::Assistant, &msg.content);
                } else {
                    let tool_calls = msg
                        .tool_calls
                        .iter()
                        .map(|tc| mistralrs::ToolCallResponse {
                            id: tc.id.to_string(),
                            tp: mistralrs::ToolCallType::Function,
                            function: mistralrs::CalledFunction {
                                name: tc.function.name.to_string(),
                                arguments: tc.function.arguments.clone(),
                            },
                            index: tc.index as usize,
                        })
                        .collect();
                    builder = builder.add_message_with_tool_call(
                        mistralrs::TextMessageRole::Assistant,
                        &msg.content,
                        tool_calls,
                    );
                }
            }
            Role::Tool => {
                builder = builder.add_tool_message(&msg.content, &msg.tool_call_id);
            }
        }
    }

    if let Some(tools) = &request.tools {
        let mr_tools = tools
            .iter()
            .map(|t| {
                let params: HashMap<String, serde_json::Value> =
                    serde_json::from_value(serde_json::to_value(&t.parameters).unwrap_or_default())
                        .unwrap_or_default();
                mistralrs::Tool {
                    tp: mistralrs::ToolType::Function,
                    function: mistralrs::Function {
                        description: Some(t.description.to_string()),
                        name: t.name.to_string(),
                        parameters: Some(params),
                    },
                }
            })
            .collect();
        builder = builder.set_tools(mr_tools);
    }

    if let Some(tool_choice) = &request.tool_choice {
        let mr_choice = match tool_choice {
            wcore::model::ToolChoice::None => mistralrs::ToolChoice::None,
            wcore::model::ToolChoice::Auto | wcore::model::ToolChoice::Required => {
                mistralrs::ToolChoice::Auto
            }
            wcore::model::ToolChoice::Function(name) => {
                mistralrs::ToolChoice::Tool(mistralrs::Tool {
                    tp: mistralrs::ToolType::Function,
                    function: mistralrs::Function {
                        description: None,
                        name: name.to_string(),
                        parameters: None,
                    },
                })
            }
        };
        builder = builder.set_tool_choice(mr_choice);
    }

    builder
}

/// Convert a mistralrs `ChatCompletionResponse` to a walrus `Response`.
fn to_response(resp: mistralrs::ChatCompletionResponse) -> Response {
    let choices = resp
        .choices
        .into_iter()
        .map(|c| Choice {
            index: c.index as u32,
            delta: Delta {
                role: Some(Role::Assistant),
                content: c.message.content,
                reasoning_content: c.message.reasoning_content,
                tool_calls: c
                    .message
                    .tool_calls
                    .map(|tcs| tcs.into_iter().map(convert_tool_call).collect()),
            },
            finish_reason: parse_finish_reason(&c.finish_reason),
            logprobs: None,
        })
        .collect();

    Response {
        meta: CompletionMeta {
            id: CompactString::from(&resp.id),
            object: CompactString::from(&resp.object),
            created: resp.created,
            model: CompactString::from(&resp.model),
            system_fingerprint: Some(CompactString::from(&resp.system_fingerprint)),
        },
        choices,
        usage: convert_usage(&resp.usage),
    }
}

/// Convert a mistralrs `ChatCompletionChunkResponse` to a walrus `StreamChunk`.
fn to_stream_chunk(chunk: mistralrs::ChatCompletionChunkResponse) -> StreamChunk {
    let choices = chunk
        .choices
        .into_iter()
        .map(|c| Choice {
            index: c.index as u32,
            delta: Delta {
                role: Some(Role::Assistant),
                content: c.delta.content,
                reasoning_content: c.delta.reasoning_content,
                tool_calls: c
                    .delta
                    .tool_calls
                    .map(|tcs| tcs.into_iter().map(convert_tool_call).collect()),
            },
            finish_reason: c
                .finish_reason
                .as_ref()
                .and_then(|r| parse_finish_reason(r)),
            logprobs: None,
        })
        .collect();

    StreamChunk {
        meta: CompletionMeta {
            id: CompactString::from(&chunk.id),
            object: CompactString::from(&chunk.object),
            created: chunk.created as u64,
            model: CompactString::from(&chunk.model),
            system_fingerprint: Some(CompactString::from(&chunk.system_fingerprint)),
        },
        choices,
        usage: chunk.usage.as_ref().map(convert_usage),
    }
}

/// Convert a mistralrs `ToolCallResponse` to a walrus `ToolCall`.
fn convert_tool_call(tc: mistralrs::ToolCallResponse) -> ToolCall {
    ToolCall {
        id: CompactString::from(&tc.id),
        index: tc.index as u32,
        call_type: CompactString::from("function"),
        function: FunctionCall {
            name: CompactString::from(&tc.function.name),
            arguments: tc.function.arguments,
        },
    }
}

/// Convert a mistralrs `Usage` to a walrus `Usage`.
fn convert_usage(u: &mistralrs::Usage) -> Usage {
    Usage {
        prompt_tokens: u.prompt_tokens as u32,
        completion_tokens: u.completion_tokens as u32,
        total_tokens: u.total_tokens as u32,
        prompt_cache_hit_tokens: None,
        prompt_cache_miss_tokens: None,
        completion_tokens_details: None,
    }
}

/// Parse a finish reason string into a walrus `FinishReason`.
fn parse_finish_reason(reason: &str) -> Option<wcore::model::FinishReason> {
    match reason {
        "stop" => Some(wcore::model::FinishReason::Stop),
        "length" => Some(wcore::model::FinishReason::Length),
        "content_filter" => Some(wcore::model::FinishReason::ContentFilter),
        "tool_calls" => Some(wcore::model::FinishReason::ToolCalls),
        _ => None,
    }
}
