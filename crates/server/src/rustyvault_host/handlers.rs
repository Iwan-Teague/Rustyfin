use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::{Json, response::IntoResponse};
use rustfin_core::error::ApiError;
use rustyvault::types::{
    ConsumeRustyVaultPairingCodeRequest, CreateRustyVaultDeviceSessionRequest,
    CreateRustyVaultDeviceSessionResponse, EncryptedRustyVaultItem, RustyVaultAuditListResponse,
    RustyVaultConfigResponse, RustyVaultDestroyRequest, RustyVaultDestroyResponse,
    RustyVaultExportRequest, RustyVaultExportResponse, RustyVaultImportBitwardenRequest,
    RustyVaultImportBitwardenResponse, RustyVaultItemListResponse, RustyVaultLookupRequest,
    RustyVaultLookupResponse, RustyVaultPreferences, RustyVaultProtectedActionChallengeRequest,
    RustyVaultProtectedActionChallengeResponse, RustyVaultRevokeOtherSessionsRequest,
    RustyVaultRevokeOtherSessionsResponse, RustyVaultSessionRefreshRequest,
    RustyVaultWrappedKeyMetadata, UpdateRustyVaultConfigRequest, UpsertRustyVaultItemRequest,
};

use crate::auth::AuthUser;
use crate::error::AppError;
use crate::rustyvault_host::auth::RustyVaultSessionUser;
use crate::rustyvault_host::{audit, device_sessions, service};
use crate::state::AppState;

#[derive(Debug, Default, serde::Deserialize)]
pub struct RustyVaultItemListQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

fn no_store_json<T: serde::Serialize>(value: T) -> impl IntoResponse {
    ([(header::CACHE_CONTROL, "no-store")], Json(value))
}

fn no_store_empty(status: StatusCode) -> impl IntoResponse {
    (status, [(header::CACHE_CONTROL, "no-store")])
}

fn touch_session_best_effort(state: &AppState, rustyvault_session: &RustyVaultSessionUser) {
    let db = state.db.clone();
    let session_id = rustyvault_session.session_id.clone();
    tokio::spawn(async move {
        let _ =
            rustfin_db::repo::rustyvault::touch_device_session(&db, &session_id, service::now_ts())
                .await;
    });
}

fn ensure_auth_matches_rustyvault_session(
    auth: &AuthUser,
    rustyvault_session: &RustyVaultSessionUser,
) -> Result<(), AppError> {
    service::ensure_rustyvault_session_user_matches(&auth.user_id, &rustyvault_session.user_id)
}

fn ensure_supported_wrapped_key(metadata: &RustyVaultWrappedKeyMetadata) -> Result<(), AppError> {
    if metadata.kdf_algorithm != service::RUSTYVAULT_KDF_ALGORITHM {
        return Err(ApiError::BadRequest("unsupported vault KDF algorithm".into()).into());
    }
    if metadata.hkdf_algorithm != service::RUSTYVAULT_HKDF_ALGORITHM {
        return Err(ApiError::BadRequest("unsupported vault HKDF algorithm".into()).into());
    }
    if metadata.wrap_algorithm != service::RUSTYVAULT_WRAP_ALGORITHM {
        return Err(ApiError::BadRequest("unsupported vault wrap algorithm".into()).into());
    }
    if metadata.kdf_memory_kib != 65_536
        || metadata.kdf_iterations != 3
        || metadata.kdf_parallelism != 4
    {
        return Err(
            ApiError::BadRequest("vault KDF profile does not match server policy".into()).into(),
        );
    }
    if metadata.key_version < 1 {
        return Err(ApiError::BadRequest("key_version must be >= 1".into()).into());
    }
    Ok(())
}

async fn ensure_target_item_owned(
    state: &AppState,
    user_id: &str,
    target_item_id: Option<&str>,
) -> Result<(), AppError> {
    let Some(target_item_id) = target_item_id else {
        return Ok(());
    };
    let exists = rustfin_db::repo::rustyvault::get_item(&state.db, user_id, target_item_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    if exists.is_none() {
        return Err(ApiError::NotFound("vault item not found".into()).into());
    }
    Ok(())
}

pub async fn get_config(
    State(state): State<AppState>,
    rustyvault_session: RustyVaultSessionUser,
) -> Result<impl IntoResponse, AppError> {
    let account = rustfin_db::repo::rustyvault::get_rustyvault_account(
        &state.db,
        &rustyvault_session.user_id,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let wrapped_key = rustfin_db::repo::rustyvault::get_active_wrapped_key(
        &state.db,
        &rustyvault_session.user_id,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let item_count =
        rustfin_db::repo::rustyvault::count_items(&state.db, &rustyvault_session.user_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let response: RustyVaultConfigResponse =
        service::build_rustyvault_config_response(account, wrapped_key, item_count);
    Ok(no_store_json(response))
}

pub async fn get_preferences(
    State(state): State<AppState>,
    rustyvault_session: RustyVaultSessionUser,
) -> Result<impl IntoResponse, AppError> {
    Ok(no_store_json(
        service::load_rustyvault_preferences(&state, &rustyvault_session.user_id).await?,
    ))
}

pub async fn update_preferences(
    State(state): State<AppState>,
    rustyvault_session: RustyVaultSessionUser,
    Json(body): Json<RustyVaultPreferences>,
) -> Result<impl IntoResponse, AppError> {
    Ok(no_store_json(
        service::save_rustyvault_preferences(&state, &rustyvault_session.user_id, body).await?,
    ))
}

pub async fn update_config(
    State(state): State<AppState>,
    rustyvault_session: RustyVaultSessionUser,
    Json(body): Json<UpdateRustyVaultConfigRequest>,
) -> Result<impl IntoResponse, AppError> {
    let display_name = service::normalize_rustyvault_display_name(&body.display_name)?;
    let account = rustfin_db::repo::rustyvault::update_rustyvault_display_name(
        &state.db,
        &rustfin_db::repo::rustyvault::UpdateRustyVaultDisplayNameInput {
            user_id: rustyvault_session.user_id.clone(),
            display_name: display_name.clone(),
            updated_ts: service::now_ts(),
        },
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let Some(account) = account else {
        return Err(ApiError::NotFound("vault not found".into()).into());
    };
    audit::record_event(
        &state,
        &rustyvault_session.user_id,
        Some(&rustyvault_session.session_id),
        "rustyvault_renamed",
        None,
        serde_json::json!({ "display_name": display_name }),
    )
    .await?;
    let wrapped_key = rustfin_db::repo::rustyvault::get_active_wrapped_key(
        &state.db,
        &rustyvault_session.user_id,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let item_count =
        rustfin_db::repo::rustyvault::count_items(&state.db, &rustyvault_session.user_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    Ok(no_store_json(service::build_rustyvault_config_response(
        Some(account),
        wrapped_key,
        item_count,
    )))
}

pub async fn bootstrap_rustyvault(
    State(state): State<AppState>,
    auth: AuthUser,
    rustyvault_session: RustyVaultSessionUser,
    Json(body): Json<rustyvault::types::RustyVaultBootstrapRequest>,
) -> Result<impl IntoResponse, AppError> {
    ensure_auth_matches_rustyvault_session(&auth, &rustyvault_session)?;
    ensure_supported_wrapped_key(&body.wrapped_key)?;
    if rustfin_db::repo::rustyvault::get_rustyvault_account(&state.db, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .is_some()
    {
        return Err(ApiError::Conflict("vault already exists".into()).into());
    }

    let wrapped_key = rustfin_db::repo::rustyvault::RustyVaultWrappedKeyInsert {
        id: uuid::Uuid::new_v4().to_string(),
        key_version: body.wrapped_key.key_version,
        kdf_algorithm: body.wrapped_key.kdf_algorithm,
        kdf_memory_kib: body.wrapped_key.kdf_memory_kib,
        kdf_iterations: body.wrapped_key.kdf_iterations,
        kdf_parallelism: body.wrapped_key.kdf_parallelism,
        kdf_salt: service::decode_hex_field("kdf_salt_hex", &body.wrapped_key.kdf_salt_hex)?,
        hkdf_algorithm: body.wrapped_key.hkdf_algorithm,
        wrap_algorithm: body.wrapped_key.wrap_algorithm,
        wrap_nonce: service::decode_hex_field("wrap_nonce_hex", &body.wrapped_key.wrap_nonce_hex)?,
        wrapped_vault_key: service::decode_hex_field(
            "wrapped_vault_key_hex",
            &body.wrapped_key.wrapped_vault_key_hex,
        )?,
        created_ts: service::now_ts(),
    };
    rustfin_db::repo::rustyvault::bootstrap_rustyvault(
        &state.db,
        &auth.user_id,
        &auth.username,
        "active",
        service::RUSTYVAULT_SCHEMA_VERSION,
        wrapped_key.key_version,
        &wrapped_key,
        service::now_ts(),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    audit::record_event(
        &state,
        &auth.user_id,
        Some(&rustyvault_session.session_id),
        "rustyvault_bootstrapped",
        None,
        serde_json::json!({}),
    )
    .await?;
    touch_session_best_effort(&state, &rustyvault_session);
    let account = rustfin_db::repo::rustyvault::get_rustyvault_account(&state.db, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let wrapped_key =
        rustfin_db::repo::rustyvault::get_active_wrapped_key(&state.db, &auth.user_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let item_count = rustfin_db::repo::rustyvault::count_items(&state.db, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    Ok(no_store_json(service::build_rustyvault_config_response(
        account,
        wrapped_key,
        item_count,
    )))
}

pub async fn rekey_rustyvault(
    State(state): State<AppState>,
    auth: AuthUser,
    rustyvault_session: RustyVaultSessionUser,
    headers: HeaderMap,
    Json(body): Json<rustyvault::types::RustyVaultBootstrapRequest>,
) -> Result<impl IntoResponse, AppError> {
    ensure_auth_matches_rustyvault_session(&auth, &rustyvault_session)?;
    ensure_supported_wrapped_key(&body.wrapped_key)?;
    service::consume_protected_action_token(
        &state,
        &auth.user_id,
        Some(rustyvault_session.session_id.as_str()),
        rustyvault::types::RustyVaultProtectedActionKind::Rekey,
        None,
        headers
            .get("x-rustyvault-protected-action")
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| {
                ApiError::BadRequest("missing x-rustyvault-protected-action header".into())
            })?,
    )
    .await?;
    touch_session_best_effort(&state, &rustyvault_session);

    let wrapped_key = rustfin_db::repo::rustyvault::RustyVaultWrappedKeyInsert {
        id: uuid::Uuid::new_v4().to_string(),
        key_version: body.wrapped_key.key_version,
        kdf_algorithm: body.wrapped_key.kdf_algorithm,
        kdf_memory_kib: body.wrapped_key.kdf_memory_kib,
        kdf_iterations: body.wrapped_key.kdf_iterations,
        kdf_parallelism: body.wrapped_key.kdf_parallelism,
        kdf_salt: service::decode_hex_field("kdf_salt_hex", &body.wrapped_key.kdf_salt_hex)?,
        hkdf_algorithm: body.wrapped_key.hkdf_algorithm,
        wrap_algorithm: body.wrapped_key.wrap_algorithm,
        wrap_nonce: service::decode_hex_field("wrap_nonce_hex", &body.wrapped_key.wrap_nonce_hex)?,
        wrapped_vault_key: service::decode_hex_field(
            "wrapped_vault_key_hex",
            &body.wrapped_key.wrapped_vault_key_hex,
        )?,
        created_ts: service::now_ts(),
    };
    rustfin_db::repo::rustyvault::rekey_rustyvault(
        &state.db,
        &auth.user_id,
        wrapped_key.key_version,
        &wrapped_key,
        service::now_ts(),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    audit::record_event(
        &state,
        &auth.user_id,
        Some(&rustyvault_session.session_id),
        "rustyvault_rekeyed",
        None,
        serde_json::json!({ "key_version": wrapped_key.key_version }),
    )
    .await?;
    let account = rustfin_db::repo::rustyvault::get_rustyvault_account(&state.db, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let active = rustfin_db::repo::rustyvault::get_active_wrapped_key(&state.db, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let item_count = rustfin_db::repo::rustyvault::count_items(&state.db, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    Ok(no_store_json(service::build_rustyvault_config_response(
        account, active, item_count,
    )))
}

pub async fn list_items(
    State(state): State<AppState>,
    rustyvault_session: RustyVaultSessionUser,
    Query(query): Query<RustyVaultItemListQuery>,
) -> Result<impl IntoResponse, AppError> {
    let limit = service::sanitize_limit(query.limit, 50);
    let offset = service::sanitize_offset(query.offset);
    let items = rustfin_db::repo::rustyvault::list_item_summaries(
        &state.db,
        &rustyvault_session.user_id,
        limit,
        offset,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let total = rustfin_db::repo::rustyvault::count_items(&state.db, &rustyvault_session.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let next_offset = if offset + limit < total {
        Some(offset + limit)
    } else {
        None
    };
    touch_session_best_effort(&state, &rustyvault_session);
    Ok(no_store_json(RustyVaultItemListResponse {
        items: items.into_iter().map(service::map_item_summary).collect(),
        next_offset,
        total,
    }))
}

pub async fn get_item(
    State(state): State<AppState>,
    rustyvault_session: RustyVaultSessionUser,
    Path(item_id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let item =
        rustfin_db::repo::rustyvault::get_item(&state.db, &rustyvault_session.user_id, &item_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .ok_or_else(|| ApiError::NotFound("vault item not found".into()))?;
    touch_session_best_effort(&state, &rustyvault_session);
    Ok(no_store_json(service::map_item(item)))
}

fn decode_upsert_item(
    body: UpsertRustyVaultItemRequest,
) -> Result<rustfin_db::repo::rustyvault::RustyVaultItemUpsert, AppError> {
    if body.id.trim().is_empty() {
        return Err(ApiError::BadRequest("vault item id is required".into()).into());
    }
    if body.item_type.trim().is_empty() {
        return Err(ApiError::BadRequest("vault item_type is required".into()).into());
    }
    if body.uri_indexes.len() > service::RUSTYVAULT_MAX_URI_INDEXES_PER_ITEM {
        return Err(ApiError::BadRequest("vault item has too many URI indexes".into()).into());
    }
    Ok(rustfin_db::repo::rustyvault::RustyVaultItemUpsert {
        id: body.id,
        item_type: body.item_type,
        key_version: body.key_version,
        summary_ciphertext: service::decode_hex_field(
            "summary_ciphertext_hex",
            &body.summary_ciphertext_hex,
        )?,
        summary_nonce: service::decode_hex_field("summary_nonce_hex", &body.summary_nonce_hex)?,
        summary_version: body.summary_version,
        payload_ciphertext: service::decode_hex_field(
            "payload_ciphertext_hex",
            &body.payload_ciphertext_hex,
        )?,
        payload_nonce: service::decode_hex_field("payload_nonce_hex", &body.payload_nonce_hex)?,
        payload_version: body.payload_version,
        favorite: body.favorite,
        revision: body.revision,
        created_ts: service::now_ts(),
        updated_ts: service::now_ts(),
        uri_indexes: body
            .uri_indexes
            .into_iter()
            .map(|index| {
                Ok(rustfin_db::repo::rustyvault::RustyVaultUriIndexInput {
                    id: uuid::Uuid::new_v4().to_string(),
                    match_hash: service::decode_hex_field("match_hash_hex", &index.match_hash_hex)?,
                    match_type: index.match_type.as_str().to_string(),
                    rank: index.rank,
                    created_ts: service::now_ts(),
                })
            })
            .collect::<Result<Vec<_>, AppError>>()?,
    })
}

pub async fn create_item(
    State(state): State<AppState>,
    rustyvault_session: RustyVaultSessionUser,
    Json(body): Json<UpsertRustyVaultItemRequest>,
) -> Result<impl IntoResponse, AppError> {
    let item = decode_upsert_item(body)?;
    rustfin_db::repo::rustyvault::upsert_item(&state.db, &rustyvault_session.user_id, &item)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    audit::record_event(
        &state,
        &rustyvault_session.user_id,
        Some(&rustyvault_session.session_id),
        "rustyvault_item_created",
        Some(&item.id),
        serde_json::json!({ "item_type": item.item_type }),
    )
    .await?;
    touch_session_best_effort(&state, &rustyvault_session);
    Ok(no_store_empty(StatusCode::CREATED))
}

pub async fn replace_item(
    State(state): State<AppState>,
    rustyvault_session: RustyVaultSessionUser,
    Path(item_id): Path<String>,
    Json(body): Json<UpsertRustyVaultItemRequest>,
) -> Result<impl IntoResponse, AppError> {
    if body.id != item_id {
        return Err(ApiError::BadRequest("item id path/body mismatch".into()).into());
    }
    if rustfin_db::repo::rustyvault::get_item(&state.db, &rustyvault_session.user_id, &item_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .is_none()
    {
        return Err(ApiError::NotFound("vault item not found".into()).into());
    }
    let item = decode_upsert_item(body)?;
    rustfin_db::repo::rustyvault::upsert_item(&state.db, &rustyvault_session.user_id, &item)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    audit::record_event(
        &state,
        &rustyvault_session.user_id,
        Some(&rustyvault_session.session_id),
        "rustyvault_item_updated",
        Some(&item.id),
        serde_json::json!({ "item_type": item.item_type }),
    )
    .await?;
    touch_session_best_effort(&state, &rustyvault_session);
    Ok(no_store_empty(StatusCode::NO_CONTENT))
}

pub async fn delete_item(
    State(state): State<AppState>,
    rustyvault_session: RustyVaultSessionUser,
    Path(item_id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let deleted = rustfin_db::repo::rustyvault::soft_delete_item(
        &state.db,
        &rustyvault_session.user_id,
        &item_id,
        service::now_ts(),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    if !deleted {
        return Err(ApiError::NotFound("vault item not found".into()).into());
    }
    audit::record_event(
        &state,
        &rustyvault_session.user_id,
        Some(&rustyvault_session.session_id),
        "rustyvault_item_deleted",
        Some(&item_id),
        serde_json::json!({}),
    )
    .await?;
    touch_session_best_effort(&state, &rustyvault_session);
    Ok(no_store_empty(StatusCode::NO_CONTENT))
}

pub async fn lookup_items(
    State(state): State<AppState>,
    rustyvault_session: RustyVaultSessionUser,
    Json(body): Json<RustyVaultLookupRequest>,
) -> Result<impl IntoResponse, AppError> {
    if body.match_hashes_hex.is_empty() {
        return Ok(no_store_json(RustyVaultLookupResponse {
            items: Vec::new(),
        }));
    }
    if body.match_hashes_hex.len() > service::RUSTYVAULT_MAX_MATCH_HASHES {
        return Err(ApiError::BadRequest("too many vault lookup hashes".into()).into());
    }
    let match_hashes = body
        .match_hashes_hex
        .into_iter()
        .map(|value| service::decode_hex_field("match_hashes_hex", &value))
        .collect::<Result<Vec<_>, AppError>>()?;
    let items = rustfin_db::repo::rustyvault::lookup_item_summaries(
        &state.db,
        &rustyvault_session.user_id,
        &match_hashes,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    touch_session_best_effort(&state, &rustyvault_session);
    Ok(no_store_json(RustyVaultLookupResponse {
        items: items.into_iter().map(service::map_item_summary).collect(),
    }))
}

pub async fn create_device_session(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    Json(body): Json<CreateRustyVaultDeviceSessionRequest>,
) -> Result<impl IntoResponse, AppError> {
    let response: CreateRustyVaultDeviceSessionResponse =
        device_sessions::create_device_session(&state, &auth.user_id, &headers, body).await?;
    Ok(no_store_json(response))
}

pub async fn consume_pairing_code(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ConsumeRustyVaultPairingCodeRequest>,
) -> Result<impl IntoResponse, AppError> {
    let response = device_sessions::consume_pairing_code(&state, &headers, body).await?;
    Ok(no_store_json(response))
}

pub async fn refresh_device_session(
    State(state): State<AppState>,
    Json(body): Json<RustyVaultSessionRefreshRequest>,
) -> Result<impl IntoResponse, AppError> {
    let response = device_sessions::refresh_device_session(&state, body).await?;
    Ok(no_store_json(response))
}

pub async fn list_device_sessions(
    State(state): State<AppState>,
    rustyvault_session: RustyVaultSessionUser,
) -> Result<impl IntoResponse, AppError> {
    let rows =
        rustfin_db::repo::rustyvault::list_device_sessions(&state.db, &rustyvault_session.user_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let sessions = rows
        .into_iter()
        .map(|row| {
            service::map_device_session_response(row, Some(rustyvault_session.session_id.as_str()))
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    Ok(no_store_json(sessions))
}

pub async fn revoke_device_session(
    State(state): State<AppState>,
    rustyvault_session: RustyVaultSessionUser,
    Path(session_id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let revoked = rustfin_db::repo::rustyvault::revoke_device_session(
        &state.db,
        &rustyvault_session.user_id,
        &session_id,
        service::now_ts(),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    if !revoked {
        return Err(ApiError::NotFound("rustyvault device session not found".into()).into());
    }
    audit::record_event(
        &state,
        &rustyvault_session.user_id,
        Some(rustyvault_session.session_id.as_str()),
        "rustyvault_device_session_revoked",
        None,
        serde_json::json!({ "session_id": session_id }),
    )
    .await?;
    Ok(no_store_empty(StatusCode::NO_CONTENT))
}

pub async fn revoke_other_device_sessions(
    State(state): State<AppState>,
    auth: AuthUser,
    rustyvault_session: RustyVaultSessionUser,
    Json(body): Json<RustyVaultRevokeOtherSessionsRequest>,
) -> Result<impl IntoResponse, AppError> {
    ensure_auth_matches_rustyvault_session(&auth, &rustyvault_session)?;
    service::consume_protected_action_token(
        &state,
        &auth.user_id,
        Some(rustyvault_session.session_id.as_str()),
        rustyvault::types::RustyVaultProtectedActionKind::RevokeOtherSessions,
        None,
        &body.protected_action_token,
    )
    .await?;

    let revoked_count = rustfin_db::repo::rustyvault::revoke_other_device_sessions(
        &state.db,
        &auth.user_id,
        Some(rustyvault_session.session_id.as_str()),
        service::now_ts(),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    audit::record_event(
        &state,
        &auth.user_id,
        Some(rustyvault_session.session_id.as_str()),
        "rustyvault_other_device_sessions_revoked",
        None,
        serde_json::json!({ "revoked_count": revoked_count }),
    )
    .await?;
    touch_session_best_effort(&state, &rustyvault_session);
    Ok(no_store_json(RustyVaultRevokeOtherSessionsResponse {
        revoked_count,
    }))
}

pub async fn challenge_protected_action(
    State(state): State<AppState>,
    auth: AuthUser,
    rustyvault_session: RustyVaultSessionUser,
    Json(body): Json<RustyVaultProtectedActionChallengeRequest>,
) -> Result<impl IntoResponse, AppError> {
    ensure_auth_matches_rustyvault_session(&auth, &rustyvault_session)?;
    ensure_target_item_owned(&state, &auth.user_id, body.target_item_id.as_deref()).await?;
    let user = rustfin_db::repo::users::find_by_id(&state.db, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::Unauthorized("user not found".into()))?;
    let valid =
        rustfin_db::repo::users::verify_password(&body.current_password, &user.password_hash)
            .map_err(|e| ApiError::Internal(format!("hash error: {e}")))?;
    if !valid {
        return Err(ApiError::Forbidden("current password is incorrect".into()).into());
    }
    let action_token = service::generate_secret_token(64);
    let token_hash = service::hash_secret(&action_token);
    let expires_ts = service::now_ts() + service::RUSTYVAULT_PROTECTED_ACTION_TTL_SECONDS;
    rustfin_db::repo::rustyvault::create_protected_action_token(
        &state.db,
        &rustfin_db::repo::rustyvault::CreateRustyVaultProtectedActionTokenInput {
            id: uuid::Uuid::new_v4().to_string(),
            user_id: auth.user_id.clone(),
            device_session_id: Some(rustyvault_session.session_id.clone()),
            action_kind: body.action_kind.as_str().to_string(),
            target_item_id: body.target_item_id.clone(),
            token_hash,
            created_ts: service::now_ts(),
            expires_ts,
        },
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    audit::record_event(
        &state,
        &auth.user_id,
        Some(rustyvault_session.session_id.as_str()),
        "rustyvault_protected_action_challenged",
        body.target_item_id.as_deref(),
        serde_json::json!({ "action_kind": body.action_kind.as_str() }),
    )
    .await?;
    touch_session_best_effort(&state, &rustyvault_session);
    Ok(no_store_json(RustyVaultProtectedActionChallengeResponse {
        action_token,
        action_kind: body.action_kind,
        expires_ts,
    }))
}

pub async fn list_audit_events(
    State(state): State<AppState>,
    rustyvault_session: RustyVaultSessionUser,
) -> Result<impl IntoResponse, AppError> {
    let rows = rustfin_db::repo::rustyvault::list_audit_events(
        &state.db,
        &rustyvault_session.user_id,
        service::RUSTYVAULT_AUDIT_LIMIT,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    Ok(no_store_json(RustyVaultAuditListResponse {
        events: rows
            .into_iter()
            .map(|row| rustyvault::types::RustyVaultAuditEventResponse {
                id: row.id,
                event_kind: row.event_kind,
                target_item_id: row.target_item_id,
                created_ts: row.created_ts,
                event_json: row.event_json,
            })
            .collect(),
    }))
}

pub async fn export_rustyvault(
    State(state): State<AppState>,
    auth: AuthUser,
    rustyvault_session: RustyVaultSessionUser,
    Json(body): Json<RustyVaultExportRequest>,
) -> Result<impl IntoResponse, AppError> {
    ensure_auth_matches_rustyvault_session(&auth, &rustyvault_session)?;
    service::consume_protected_action_token(
        &state,
        &auth.user_id,
        Some(rustyvault_session.session_id.as_str()),
        rustyvault::types::RustyVaultProtectedActionKind::Export,
        None,
        &body.protected_action_token,
    )
    .await?;
    touch_session_best_effort(&state, &rustyvault_session);
    let wrapped_key =
        rustfin_db::repo::rustyvault::get_active_wrapped_key(&state.db, &auth.user_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let item_count = rustfin_db::repo::rustyvault::count_items(&state.db, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let items: Vec<EncryptedRustyVaultItem> =
        rustfin_db::repo::rustyvault::list_all_items(&state.db, &auth.user_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .into_iter()
            .map(service::map_item)
            .collect();
    audit::record_event(
        &state,
        &auth.user_id,
        Some(&rustyvault_session.session_id),
        "rustyvault_export_requested",
        None,
        serde_json::json!({ "item_count": items.len() }),
    )
    .await?;
    Ok(no_store_json(RustyVaultExportResponse {
        config: service::build_rustyvault_config_response(
            rustfin_db::repo::rustyvault::get_rustyvault_account(&state.db, &auth.user_id)
                .await
                .map_err(|e| ApiError::Internal(format!("db error: {e}")))?,
            wrapped_key,
            item_count,
        ),
        items,
    }))
}

pub async fn import_bitwarden(
    State(state): State<AppState>,
    auth: AuthUser,
    rustyvault_session: RustyVaultSessionUser,
    Json(body): Json<RustyVaultImportBitwardenRequest>,
) -> Result<impl IntoResponse, AppError> {
    ensure_auth_matches_rustyvault_session(&auth, &rustyvault_session)?;
    if body.items.len() > service::RUSTYVAULT_MAX_IMPORT_ITEMS {
        return Err(ApiError::BadRequest("import exceeds vault item limit".into()).into());
    }
    service::consume_protected_action_token(
        &state,
        &auth.user_id,
        Some(rustyvault_session.session_id.as_str()),
        rustyvault::types::RustyVaultProtectedActionKind::ImportOverwrite,
        None,
        &body.protected_action_token,
    )
    .await?;
    touch_session_best_effort(&state, &rustyvault_session);

    if body.clear_existing {
        rustfin_db::repo::rustyvault::clear_all_items(&state.db, &auth.user_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    }

    let mut imported_count = 0usize;
    for item in body.items {
        let decoded = decode_upsert_item(item)?;
        rustfin_db::repo::rustyvault::upsert_item(&state.db, &auth.user_id, &decoded)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
        imported_count += 1;
    }
    audit::record_event(
        &state,
        &auth.user_id,
        Some(&rustyvault_session.session_id),
        "rustyvault_import_completed",
        None,
        serde_json::json!({ "imported_count": imported_count, "cleared_existing": body.clear_existing }),
    )
    .await?;
    Ok(no_store_json(RustyVaultImportBitwardenResponse {
        imported_count,
        cleared_existing: body.clear_existing,
    }))
}

pub async fn destroy_rustyvault(
    State(state): State<AppState>,
    auth: AuthUser,
    rustyvault_session: RustyVaultSessionUser,
    Json(body): Json<RustyVaultDestroyRequest>,
) -> Result<impl IntoResponse, AppError> {
    ensure_auth_matches_rustyvault_session(&auth, &rustyvault_session)?;
    service::consume_protected_action_token(
        &state,
        &auth.user_id,
        Some(rustyvault_session.session_id.as_str()),
        rustyvault::types::RustyVaultProtectedActionKind::DestroyRustyVault,
        None,
        &body.protected_action_token,
    )
    .await?;
    rustfin_db::repo::rustyvault::destroy_rustyvault(&state.db, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    Ok(no_store_json(RustyVaultDestroyResponse { destroyed: true }))
}
