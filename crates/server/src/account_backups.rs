use std::io::{Cursor, Write};

use axum::Json;
use axum::body::Body;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::Response;
use rustfin_core::error::ApiError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

use crate::auth::AuthUser;
use crate::error::AppError;
use crate::state::AppState;
use crate::user_activity::{self, ActivityRange};

#[derive(Debug, Default, Deserialize)]
pub struct CreateAccountArchiveRequest {
    #[serde(default)]
    pub vault_export_json: Option<Value>,
    #[serde(default)]
    pub vault_preferences_json: Option<Value>,
}

#[derive(Debug, Serialize)]
struct AccountBackupManifest {
    format: &'static str,
    version: i32,
    exported_at: i64,
    user_id: String,
    username: String,
    includes_vault_snapshot: bool,
    files: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct AccountBackupProfile {
    id: String,
    username: String,
    display_name: String,
    time_zone: Option<String>,
    avatar_path: Option<String>,
    avatar_content_type: Option<String>,
    role: String,
    created_ts: i64,
}

#[derive(Debug, Serialize)]
struct AccountBackupStats {
    ai_conversation_count: usize,
    ai_turn_count: usize,
    play_state_count: usize,
    continue_watching_count: usize,
    activity_session_count: usize,
    activity_daily_count: usize,
}

#[derive(Debug, Serialize)]
struct AccountBackupConversation {
    id: String,
    title: String,
    archived: bool,
    group_name: Option<String>,
    sort_order: i64,
    last_message_preview: Option<String>,
    last_model_name: Option<String>,
    memory_state_json: String,
    memory_turn_index: i64,
    memory_updated_ts: Option<i64>,
    created_ts: i64,
    updated_ts: i64,
    turns: Vec<AccountBackupConversationTurn>,
}

#[derive(Debug, Serialize)]
struct AccountBackupConversationTurn {
    id: String,
    turn_index: i64,
    role: String,
    content: String,
    model_name: Option<String>,
    grounding_tools_json: String,
    follow_up_contexts_json: String,
    grounding_chunks_json: String,
    grounding_sources_json: String,
    activity_trace_json: String,
    stats_json: Option<String>,
    pending_action_json: Option<String>,
    trace_id: Option<String>,
    created_ts: i64,
}

#[derive(Debug, Serialize)]
struct AccountBackupPlayState {
    item_id: String,
    library_id: String,
    kind: String,
    title: String,
    year: Option<i64>,
    progress_ms: i64,
    last_played_ts: Option<i64>,
    played: bool,
    favorite: bool,
}

#[derive(Debug, Serialize)]
struct AccountBackupContinueWatchingRow {
    item_id: String,
    library_id: String,
    kind: String,
    title: String,
    year: Option<i64>,
    poster_url: Option<String>,
    thumb_url: Option<String>,
    progress_ms: i64,
    duration_ms: Option<i64>,
    last_played_ts: i64,
}

#[derive(Debug, Serialize)]
struct AccountBackupActivityRow {
    activity_kind: String,
    section_key: String,
    subject_type: String,
    subject_id: String,
    started_ts: i64,
    last_heartbeat_ts: i64,
    ended_ts: Option<i64>,
    accumulated_ms: i64,
}

fn is_missing_table(error: &sqlx::Error) -> bool {
    match error {
        sqlx::Error::Database(db_error) => db_error.code().as_deref() == Some("42P01"),
        _ => false,
    }
}

fn zip_options() -> SimpleFileOptions {
    SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o600)
}

fn add_json_entry<T: Serialize>(
    writer: &mut zip::ZipWriter<Cursor<Vec<u8>>>,
    path: &str,
    value: &T,
) -> Result<(), AppError> {
    writer.start_file(path, zip_options()).map_err(|error| {
        ApiError::Internal(format!("failed to add archive entry {path}: {error}"))
    })?;
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| {
        ApiError::Internal(format!("failed to serialize archive entry {path}: {error}"))
    })?;
    writer.write_all(&bytes).map_err(|error| {
        ApiError::Internal(format!("failed to write archive entry {path}: {error}"))
    })?;
    Ok(())
}

fn add_text_entry(
    writer: &mut zip::ZipWriter<Cursor<Vec<u8>>>,
    path: &str,
    value: &str,
) -> Result<(), AppError> {
    writer.start_file(path, zip_options()).map_err(|error| {
        ApiError::Internal(format!("failed to add archive entry {path}: {error}"))
    })?;
    writer.write_all(value.as_bytes()).map_err(|error| {
        ApiError::Internal(format!("failed to write archive entry {path}: {error}"))
    })?;
    Ok(())
}

pub async fn create_account_archive(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<CreateAccountArchiveRequest>,
) -> Result<Response, AppError> {
    let user = rustfin_db::repo::users::find_by_id(&state.db, &auth.user_id)
        .await
        .map_err(|error| ApiError::Internal(format!("db error: {error}")))?
        .ok_or_else(|| ApiError::NotFound("user not found".into()))?;

    let preferences = user_activity::load_preferences(&state, &auth.user_id).await?;
    let activity_summary =
        user_activity::summarize_user_activity(&state, &auth.user_id, ActivityRange::ThirtyDays)
            .await?;
    let activity_daily =
        rustfin_db::repo::user_activity::get_daily_rows_for_user(&state.db, &auth.user_id)
            .await
            .map_err(|error| ApiError::Internal(format!("db error: {error}")))?;
    let activity_history = rustfin_db::repo::user_activity::list_activity_rows_for_range(
        &state.db,
        &auth.user_id,
        None,
    )
    .await
    .map_err(|error| ApiError::Internal(format!("db error: {error}")))?;

    let play_states =
        rustfin_db::repo::playstate::list_play_states_for_user(&state.db, &auth.user_id)
            .await
            .map_err(|error| ApiError::Internal(format!("db error: {error}")))?;
    let continue_watching =
        rustfin_db::repo::playstate::list_continue_watching(&state.db, &auth.user_id, None, 250)
            .await
            .map_err(|error| ApiError::Internal(format!("db error: {error}")))?;

    let ai_conversations_raw =
        match rustfin_db::repo::ai_conversations::list_conversations_for_user(
            &state.db,
            &auth.user_id,
            true,
            500,
        )
        .await
        {
            Ok(rows) => rows,
            Err(error) if is_missing_table(&error) => Vec::new(),
            Err(error) => return Err(ApiError::Internal(format!("db error: {error}")).into()),
        };

    let mut ai_turn_count = 0usize;
    let mut ai_conversations = Vec::with_capacity(ai_conversations_raw.len());
    for conversation in ai_conversations_raw {
        let turns = rustfin_db::repo::ai_conversations::list_turns_for_conversation(
            &state.db,
            &conversation.id,
            &auth.user_id,
        )
        .await
        .map_err(|error| ApiError::Internal(format!("db error: {error}")))?;
        ai_turn_count += turns.len();
        ai_conversations.push(AccountBackupConversation {
            id: conversation.id,
            title: conversation.title,
            archived: conversation.archived,
            group_name: conversation.group_name,
            sort_order: conversation.sort_order,
            last_message_preview: conversation.last_message_preview,
            last_model_name: conversation.last_model_name,
            memory_state_json: conversation.memory_state_json,
            memory_turn_index: conversation.memory_turn_index,
            memory_updated_ts: conversation.memory_updated_ts,
            created_ts: conversation.created_ts,
            updated_ts: conversation.updated_ts,
            turns: turns
                .into_iter()
                .map(|turn| AccountBackupConversationTurn {
                    id: turn.id,
                    turn_index: turn.turn_index,
                    role: turn.role,
                    content: turn.content,
                    model_name: turn.model_name,
                    grounding_tools_json: turn.grounding_tools_json,
                    follow_up_contexts_json: turn.follow_up_contexts_json,
                    grounding_chunks_json: turn.grounding_chunks_json,
                    grounding_sources_json: turn.grounding_sources_json,
                    activity_trace_json: turn.activity_trace_json,
                    stats_json: turn.stats_json,
                    pending_action_json: turn.pending_action_json,
                    trace_id: turn.trace_id,
                    created_ts: turn.created_ts,
                })
                .collect(),
        });
    }

    let play_state_export = play_states
        .into_iter()
        .map(|row| AccountBackupPlayState {
            item_id: row.item_id,
            library_id: row.library_id,
            kind: row.kind,
            title: row.title,
            year: row.year,
            progress_ms: row.progress_ms,
            last_played_ts: row.last_played_ts,
            played: row.played,
            favorite: row.favorite,
        })
        .collect::<Vec<_>>();
    let continue_watching_export = continue_watching
        .into_iter()
        .map(|row| AccountBackupContinueWatchingRow {
            item_id: row.item_id,
            library_id: row.library_id,
            kind: row.kind,
            title: row.title,
            year: row.year,
            poster_url: row.poster_url,
            thumb_url: row.thumb_url,
            progress_ms: row.progress_ms,
            duration_ms: row.duration_ms,
            last_played_ts: row.last_played_ts,
        })
        .collect::<Vec<_>>();
    let activity_history_export = activity_history
        .into_iter()
        .map(|row| AccountBackupActivityRow {
            activity_kind: row.activity_kind,
            section_key: row.section_key,
            subject_type: row.subject_type,
            subject_id: row.subject_id,
            started_ts: row.started_ts,
            last_heartbeat_ts: row.last_heartbeat_ts,
            ended_ts: row.ended_ts,
            accumulated_ms: row.accumulated_ms,
        })
        .collect::<Vec<_>>();

    let stats = AccountBackupStats {
        ai_conversation_count: ai_conversations.len(),
        ai_turn_count,
        play_state_count: play_state_export.len(),
        continue_watching_count: continue_watching_export.len(),
        activity_session_count: activity_history_export.len(),
        activity_daily_count: activity_daily.len(),
    };

    let profile = AccountBackupProfile {
        id: user.id,
        username: user.username.clone(),
        display_name: user.display_name,
        time_zone: user.time_zone,
        avatar_path: user.avatar_path,
        avatar_content_type: user.avatar_content_type,
        role: user.role,
        created_ts: user.created_ts,
    };

    let mut files = vec![
        "README.md",
        "manifest.json",
        "account/profile.json",
        "account/preferences.json",
        "account/stats.json",
        "ai/conversations.json",
        "activity/summary-30d.json",
        "activity/daily-rollups.json",
        "activity/history.json",
        "playback/play-states.json",
        "playback/continue-watching.json",
    ];
    if body.vault_export_json.is_some() {
        files.push("vault/export.json");
    }
    if body.vault_preferences_json.is_some() {
        files.push("vault/preferences.json");
    }

    let exported_at = chrono::Utc::now().timestamp();
    let manifest = AccountBackupManifest {
        format: "rustyfin-account-backup",
        version: 1,
        exported_at,
        user_id: auth.user_id.clone(),
        username: user.username.clone(),
        includes_vault_snapshot: body.vault_export_json.is_some(),
        files,
    };

    let readme = format!(
        "# Rustyfin Account Backup\n\n\
Exported at: {exported_at}\n\
Username: {}\n\n\
This archive contains a user-scoped snapshot of profile state, preferences, AI chat history, watch history, continue-watching state, and activity rollups.\n\n\
If `vault/export.json` is present, it is the RustyVault export payload captured through the protected vault export flow so it can be loaded back into place later.\n",
        user.username
    );

    let cursor = Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(cursor);
    add_text_entry(&mut writer, "README.md", &readme)?;
    add_json_entry(&mut writer, "manifest.json", &manifest)?;
    add_json_entry(&mut writer, "account/profile.json", &profile)?;
    add_json_entry(&mut writer, "account/preferences.json", &preferences)?;
    add_json_entry(&mut writer, "account/stats.json", &stats)?;
    add_json_entry(&mut writer, "ai/conversations.json", &ai_conversations)?;
    add_json_entry(&mut writer, "activity/summary-30d.json", &activity_summary)?;
    add_json_entry(&mut writer, "activity/daily-rollups.json", &activity_daily)?;
    add_json_entry(
        &mut writer,
        "activity/history.json",
        &activity_history_export,
    )?;
    add_json_entry(&mut writer, "playback/play-states.json", &play_state_export)?;
    add_json_entry(
        &mut writer,
        "playback/continue-watching.json",
        &continue_watching_export,
    )?;
    if let Some(vault_export_json) = &body.vault_export_json {
        add_json_entry(&mut writer, "vault/export.json", vault_export_json)?;
    }
    if let Some(vault_preferences_json) = &body.vault_preferences_json {
        add_json_entry(
            &mut writer,
            "vault/preferences.json",
            vault_preferences_json,
        )?;
    }

    let bytes = writer
        .finish()
        .map_err(|error| {
            ApiError::Internal(format!(
                "failed to finalize account backup archive: {error}"
            ))
        })?
        .into_inner();

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::CONTENT_TYPE, "application/zip")
        .header(
            header::CONTENT_DISPOSITION,
            format!(
                "attachment; filename=\"rustyfin-account-backup-{}-{}.zip\"",
                user.username.replace('"', ""),
                chrono::Utc::now().format("%Y-%m-%d")
            ),
        )
        .body(Body::from(bytes))
        .map_err(|error| {
            ApiError::Internal(format!("failed to build account backup response: {error}")).into()
        })
}
