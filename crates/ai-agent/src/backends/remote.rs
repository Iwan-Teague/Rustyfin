use std::time::Instant;

use async_stream::stream;
use futures::StreamExt;
use futures::stream::BoxStream;
use serde_json::Value;

use crate::SamplingParams;
use crate::backend::RemoteBackendConfig;
use crate::error::AiError;
use crate::types::{BackendCapabilities, BackendKind, ChatChunk, ChatMessage, PromptCacheHint};

pub struct RemotePromptBackend {
    config: RemoteBackendConfig,
    client: reqwest::Client,
}

impl RemotePromptBackend {
    pub fn new(config: RemoteBackendConfig) -> Result<Self, AiError> {
        config.validate()?;
        let client = reqwest::Client::builder()
            .timeout(config.timeout())
            .build()
            .map_err(|error| {
                AiError::ContextError(format!("failed to build remote client: {error}"))
            })?;
        Ok(Self { config, client })
    }

    pub fn config(&self) -> &RemoteBackendConfig {
        &self.config
    }
}

impl super::PromptBackend for RemotePromptBackend {
    fn backend_kind(&self) -> BackendKind {
        BackendKind::Remote
    }

    fn capabilities(&self) -> BackendCapabilities {
        self.config.capabilities()
    }

    fn chat_stream_boxed(
        &self,
        messages: Vec<ChatMessage>,
        sampling: SamplingParams,
        prompt_cache: Option<PromptCacheHint>,
    ) -> BoxStream<'static, Result<ChatChunk, AiError>> {
        let client = self.client.clone();
        let config = self.config.clone();

        Box::pin(stream! {
            let started = Instant::now();
            let request_body = build_chat_request_body(&config, &messages, sampling, prompt_cache);

            let request = client.post(config.base_url.trim()).json(&request_body);
            let request = if let Some(api_key) = config.api_key() {
                request.bearer_auth(api_key)
            } else {
                request
            };

            let response = match request.send().await {
                Ok(response) => response,
                Err(error) => {
                    yield Err(AiError::InferenceError(format!("remote backend request failed: {error}")));
                    return;
                }
            };

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                yield Err(AiError::InferenceError(format!(
                    "remote backend returned {status}: {body}"
                )));
                return;
            }

            let mut stream = response.bytes_stream();
            let mut buffer = String::new();
            let mut prompt_tokens = 0_u64;
            let mut completion_tokens = 0_u64;
            let mut first_token_ms: Option<u64> = None;
            let mut saw_token = false;

            while let Some(next) = stream.next().await {
                let chunk = match next {
                    Ok(chunk) => chunk,
                    Err(error) => {
                        yield Err(AiError::InferenceError(format!(
                            "remote backend stream failed: {error}"
                        )));
                        return;
                    }
                };

                buffer.push_str(&String::from_utf8_lossy(&chunk));
                while let Some(boundary) = buffer.find("\n\n") {
                    let block = buffer[..boundary].to_string();
                    buffer = buffer[boundary + 2..].to_string();
                    if let Some(text) = parse_stream_block(&block) {
                        if !text.is_empty() {
                            if first_token_ms.is_none() {
                                first_token_ms = Some(started.elapsed().as_millis() as u64);
                            }
                            completion_tokens = completion_tokens.saturating_add(1);
                            saw_token = true;
                            yield Ok(ChatChunk::Token(text));
                        }
                    }
                    if let Some((prompt, completion)) = parse_usage_block(&block) {
                        prompt_tokens = prompt_tokens.max(prompt);
                        completion_tokens = completion_tokens.max(completion);
                    }
                }
            }

            if !buffer.trim().is_empty() {
                if let Some(text) = parse_stream_payload(&buffer) {
                    if !text.is_empty() {
                        if first_token_ms.is_none() {
                            first_token_ms = Some(started.elapsed().as_millis() as u64);
                        }
                        completion_tokens = completion_tokens.saturating_add(1);
                        saw_token = true;
                        yield Ok(ChatChunk::Token(text));
                    }
                } else if let Some((prompt, completion)) = parse_usage_block(&buffer) {
                    prompt_tokens = prompt_tokens.max(prompt);
                    completion_tokens = completion_tokens.max(completion);
                }
            }

            let total_duration_ms = started.elapsed().as_millis() as u64;
            let first_token_ms = first_token_ms.unwrap_or(total_duration_ms);
            let seconds = total_duration_ms as f64 / 1000.0;
            let tokens_per_second = if completion_tokens > 0 && seconds > 0.0 {
                completion_tokens as f64 / seconds
            } else {
                0.0
            };

            if !saw_token {
                yield Ok(ChatChunk::Token(String::new()));
            }

            yield Ok(ChatChunk::Stats {
                prompt_tokens,
                completion_tokens,
                prefill_duration_ms: first_token_ms,
                total_duration_ms,
                tokens_per_second,
            });
            yield Ok(ChatChunk::Done);
        })
    }
}

fn build_chat_request_body(
    config: &RemoteBackendConfig,
    messages: &[ChatMessage],
    sampling: SamplingParams,
    prompt_cache: Option<PromptCacheHint>,
) -> serde_json::Value {
    let mut body = serde_json::json!({
        "model": config.model,
        "messages": messages,
        "stream": true,
        "temperature": if sampling.temperature.is_finite() { sampling.temperature.max(0.0) } else { 0.8 },
        "top_p": if sampling.top_p.is_finite() { sampling.top_p.clamp(0.0, 1.0) } else { 0.95 },
        "max_tokens": sampling.max_tokens,
    });

    if let Some(cache_hint) = prompt_cache {
        if cache_hint.enabled && config.supports_prompt_cache {
            body["metadata"] = serde_json::json!({
                "rustyfin_prompt_cache": {
                    "cache_key": cache_hint.cache_key,
                    "cache_scope": cache_hint.cache_scope,
                    "enabled": cache_hint.enabled,
                }
            });
        }
    }

    body
}

fn parse_stream_block(block: &str) -> Option<String> {
    let data = block
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim_start();
            trimmed
                .strip_prefix("data:")
                .map(|value| value.trim_start())
        })
        .collect::<Vec<_>>()
        .join("\n");
    parse_stream_payload(&data)
}

fn parse_stream_payload(payload: &str) -> Option<String> {
    let payload = payload.trim();
    if payload.is_empty() || payload == "[DONE]" {
        return None;
    }

    let value = serde_json::from_str::<Value>(payload).ok()?;
    value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| {
            choice
                .get("delta")
                .and_then(|delta| delta.get("content"))
                .and_then(Value::as_str)
                .or_else(|| choice.get("text").and_then(Value::as_str))
                .or_else(|| {
                    choice
                        .get("message")
                        .and_then(|message| message.get("content"))
                        .and_then(Value::as_str)
                })
        })
        .map(str::to_string)
}

fn parse_usage_block(block: &str) -> Option<(u64, u64)> {
    let payload = if block.contains("data:") {
        block
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim_start();
                trimmed
                    .strip_prefix("data:")
                    .map(|value| value.trim_start())
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        block.trim().to_string()
    };

    if payload.is_empty() || payload == "[DONE]" {
        return None;
    }

    let value = serde_json::from_str::<Value>(&payload).ok()?;
    let usage = value.get("usage")?;
    let prompt_tokens = usage
        .get("prompt_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let completion_tokens = usage
        .get("completion_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    Some((prompt_tokens, completion_tokens))
}
