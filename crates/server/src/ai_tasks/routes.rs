use std::convert::Infallible;
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response, Sse, sse::Event, sse::KeepAlive};
use axum::routing::{get, post};
use rustfin_core::error::ApiError;

use crate::auth::AuthUser;
use crate::error::AppError;
use crate::state::AppState;

use super::events::append_task_event;
use super::scheduler::enqueue_task;
use super::store::{AiTaskStore, DbAiTaskStore};
use super::types::{
    AiTaskEventsQuery, AiTaskEventsResponse, AiTaskListResponse, CreateAiTaskRequest,
    TaskUserContext,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/tasks", post(create_task).get(list_tasks))
        .route("/tasks/{id}", get(get_task))
        .route("/tasks/{id}/events", get(get_task_events))
        .route("/tasks/{id}/cancel", post(cancel_task))
        .route("/tasks/{id}/resume", post(resume_task))
        .route(
            "/tasks/{id}/artifacts/{artifact_id}/download",
            get(download_task_artifact),
        )
}

async fn create_task(
    user: AuthUser,
    State(state): State<AppState>,
    Json(request): Json<CreateAiTaskRequest>,
) -> Result<impl IntoResponse, AppError> {
    let store = DbAiTaskStore::new(state.db.clone());
    let task = store
        .create_task(&TaskUserContext::from(&user), &request)
        .await
        .map_err(ApiError::Internal)?;
    append_task_event(
        &store,
        &task.id,
        "task_created",
        &serde_json::json!({
            "task_type": task.task_type,
            "requested_model": task.requested_model,
        }),
    )
    .await
    .map_err(ApiError::Internal)?;
    enqueue_task(state, task.id.clone());
    Ok((StatusCode::ACCEPTED, Json(task)))
}

async fn list_tasks(
    user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<AiTaskListResponse>, AppError> {
    let store = DbAiTaskStore::new(state.db.clone());
    let tasks = store
        .list_tasks_for_user(&user.user_id)
        .await
        .map_err(ApiError::Internal)?;
    Ok(Json(AiTaskListResponse { tasks }))
}

async fn get_task(
    user: AuthUser,
    State(state): State<AppState>,
    Path(task_id): Path<String>,
) -> Result<Json<super::types::AiTaskRecord>, AppError> {
    let store = DbAiTaskStore::new(state.db.clone());
    let task = store
        .get_task_for_user(&task_id, &user.user_id)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound("AI task not found".into()))?;
    Ok(Json(task))
}

async fn cancel_task(
    user: AuthUser,
    State(state): State<AppState>,
    Path(task_id): Path<String>,
) -> Result<Json<super::types::AiTaskRecord>, AppError> {
    let store = DbAiTaskStore::new(state.db.clone());
    let task = store
        .request_cancel(&task_id, &user.user_id)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound("AI task not found".into()))?;
    let event_type = if task.status == super::types::AiTaskStatus::Cancelled {
        "task_cancelled"
    } else {
        "cancel_requested"
    };
    append_task_event(
        &store,
        &task_id,
        event_type,
        &serde_json::json!({ "status": task.status, "phase": task.phase }),
    )
    .await
    .map_err(ApiError::Internal)?;
    Ok(Json(task))
}

async fn resume_task(
    user: AuthUser,
    State(state): State<AppState>,
    Path(task_id): Path<String>,
) -> Result<Json<super::types::AiTaskRecord>, AppError> {
    let store = DbAiTaskStore::new(state.db.clone());
    let Some(task) = store
        .resume_task(&task_id, &user.user_id)
        .await
        .map_err(ApiError::Internal)?
    else {
        return Err(
            ApiError::Conflict("AI task cannot be resumed from its current state".into()).into(),
        );
    };
    append_task_event(
        &store,
        &task_id,
        "task_resumed",
        &serde_json::json!({ "status": task.status, "phase": task.phase }),
    )
    .await
    .map_err(ApiError::Internal)?;
    enqueue_task(state, task.id.clone());
    Ok(Json(task))
}

async fn get_task_events(
    user: AuthUser,
    State(state): State<AppState>,
    Path(task_id): Path<String>,
    Query(query): Query<AiTaskEventsQuery>,
) -> Result<Response, AppError> {
    let store = DbAiTaskStore::new(state.db.clone());
    if store
        .get_task_for_user(&task_id, &user.user_id)
        .await
        .map_err(ApiError::Internal)?
        .is_none()
    {
        return Err(ApiError::NotFound("AI task not found".into()).into());
    }

    if !query.stream {
        let events = store
            .list_events_for_user(&task_id, &user.user_id, query.after_id)
            .await
            .map_err(ApiError::Internal)?;
        return Ok(Json(AiTaskEventsResponse { task_id, events }).into_response());
    }

    let state_for_stream = state.clone();
    let task_id_for_stream = task_id.clone();
    let user_id = user.user_id.clone();
    let mut cursor = query.after_id.unwrap_or(0);
    let stream = async_stream::stream! {
        loop {
            let store = DbAiTaskStore::new(state_for_stream.db.clone());
            match store
                .list_events_for_user(&task_id_for_stream, &user_id, Some(cursor))
                .await
            {
                Ok(events) => {
                    for event in events {
                        cursor = event.id;
                        let data = serde_json::to_string(&event).unwrap_or_else(|_| "{}".to_string());
                        yield Ok::<Event, Infallible>(Event::default().event(&event.event_type).data(data));
                    }
                }
                Err(error) => {
                    let payload = serde_json::json!({ "message": error });
                    yield Ok::<Event, Infallible>(Event::default().event("error").data(payload.to_string()));
                    break;
                }
            }

            match store.get_task_for_user(&task_id_for_stream, &user_id).await {
                Ok(Some(task)) if matches!(
                    task.status,
                    super::types::AiTaskStatus::Completed
                        | super::types::AiTaskStatus::Failed
                        | super::types::AiTaskStatus::Cancelled
                ) => break,
                Ok(Some(_)) => {}
                Ok(None) => break,
                Err(error) => {
                    let payload = serde_json::json!({ "message": error });
                    yield Ok::<Event, Infallible>(Event::default().event("error").data(payload.to_string()));
                    break;
                }
            }

            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    };

    Ok(Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(10)))
        .into_response())
}

async fn download_task_artifact(
    user: AuthUser,
    State(state): State<AppState>,
    Path((task_id, artifact_id)): Path<(String, String)>,
) -> Result<Response, AppError> {
    let store = DbAiTaskStore::new(state.db.clone());
    let artifact = store
        .get_artifact_for_user(&task_id, &artifact_id, &user.user_id)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound("AI task artifact not found".into()))?;

    let bytes = tokio::fs::read(&artifact.storage_path)
        .await
        .map_err(|e| ApiError::Internal(format!("failed to read AI task artifact: {e}")))?;

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CACHE_CONTROL, "no-store")
        .header(
            header::CONTENT_TYPE,
            sanitize_media_type(&artifact.media_type),
        )
        .header(
            header::CONTENT_DISPOSITION,
            format!(
                "attachment; filename=\"{}\"",
                artifact.file_name.replace('"', "")
            ),
        )
        .body(Body::from(bytes))
        .map_err(|e| {
            ApiError::Internal(format!("failed to build AI task artifact response: {e}")).into()
        })
}

fn sanitize_media_type(media_type: &str) -> &'static str {
    match media_type {
        "text/markdown; charset=utf-8" => "text/markdown; charset=utf-8",
        "text/plain; charset=utf-8" => "text/plain; charset=utf-8",
        "application/json; charset=utf-8" => "application/json; charset=utf-8",
        _ => "application/octet-stream",
    }
}
