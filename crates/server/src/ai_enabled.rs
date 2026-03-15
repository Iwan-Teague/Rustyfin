use std::convert::Infallible;

use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::{StreamExt, stream};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::ai_storage::{
    AiModelSummary, current_model_dir, list_models_from_state, model_file_path,
};
use crate::auth::AuthUser;
use crate::error::AppError;
use crate::state::AppState;

pub struct EngineState {
    pub loaded_model: Option<String>,
    pub engine: Option<rustfin_ai_agent::LlamaEngine>,
}

impl Default for EngineState {
    fn default() -> Self {
        Self {
            loaded_model: None,
            engine: None,
        }
    }
}

pub fn ai_router() -> Router<AppState> {
    Router::new()
        .route("/models", get(list_models))
        .route("/chat", post(chat))
}

fn sse_error(message: impl Into<String>) -> Response {
    let event = Event::default()
        .event("error")
        .data(json!({ "message": message.into() }).to_string());
    let stream = stream::once(async move { Ok::<Event, Infallible>(event) });
    Sse::new(Box::pin(stream))
        .keep_alive(KeepAlive::default())
        .into_response()
}

#[derive(Serialize)]
struct ModelsResponse {
    models: Vec<AiModelSummary>,
    inference_available: bool,
}

async fn list_models(
    _user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<ModelsResponse>, AppError> {
    let models = list_models_from_state(&state).await?;

    Ok(Json(ModelsResponse {
        models,
        inference_available: true,
    }))
}

#[derive(Deserialize)]
struct ChatRequest {
    model: String,
    message: String,
    #[serde(default)]
    history: Vec<HistoryMessage>,
}

#[derive(Deserialize)]
struct HistoryMessage {
    role: String,
    content: String,
}

async fn chat(
    _user: AuthUser,
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> Response {
    let system_prompt =
        "You are the Rustyfin assistant — a helpful AI built into a personal home media server. \
         Be concise and genuinely helpful. Respond in plain text unless code or markdown lists \
         add real clarity to the answer."
            .to_string();

    let mut messages = vec![rustfin_ai_agent::ChatMessage {
        role: "system".to_string(),
        content: system_prompt,
    }];

    for history in req.history {
        messages.push(rustfin_ai_agent::ChatMessage {
            role: history.role,
            content: history.content,
        });
    }

    messages.push(rustfin_ai_agent::ChatMessage {
        role: "user".to_string(),
        content: req.message,
    });

    let model_dir = current_model_dir(&state).await;
    let gguf_path = match model_file_path(&model_dir, &req.model) {
        Ok(path) => path,
        Err(error) => return sse_error(error.to_string()),
    };

    let engine = {
        let mut guard = state.engine.lock().await;
        let needs_reload =
            guard.engine.is_none() || guard.loaded_model.as_deref() != Some(req.model.as_str());

        if needs_reload {
            match rustfin_ai_agent::LlamaEngine::load(
                &gguf_path,
                rustfin_ai_agent::LlamaEngineParams::default(),
            ) {
                Ok(engine) => {
                    guard.loaded_model = Some(req.model.clone());
                    guard.engine = Some(engine);
                }
                Err(error) => {
                    return sse_error(format!(
                        "failed to load model {}: {error}",
                        gguf_path.display()
                    ));
                }
            }
        }

        guard.engine.clone()
    };

    let Some(engine) = engine else {
        return sse_error("no inference engine loaded");
    };

    let raw_stream = engine.chat_stream(messages, rustfin_ai_agent::SamplingParams::default());
    let sse_stream = raw_stream.map(|chunk| {
        let event = match chunk {
            Ok(rustfin_ai_agent::ChatChunk::Token(text)) => Event::default()
                .event("token")
                .data(json!({ "text": text }).to_string()),
            Ok(rustfin_ai_agent::ChatChunk::Stats {
                prompt_tokens,
                completion_tokens,
                total_duration_ms,
                tokens_per_second,
            }) => Event::default().event("stats").data(
                json!({
                    "prompt_tokens": prompt_tokens,
                    "completion_tokens": completion_tokens,
                    "total_duration_ms": total_duration_ms,
                    "tokens_per_second": tokens_per_second,
                })
                .to_string(),
            ),
            Ok(rustfin_ai_agent::ChatChunk::Done) => Event::default().event("done").data("{}"),
            Err(error) => Event::default()
                .event("error")
                .data(json!({ "message": error.to_string() }).to_string()),
        };
        Ok::<Event, Infallible>(event)
    });

    Sse::new(Box::pin(sse_stream))
        .keep_alive(KeepAlive::default())
        .into_response()
}
