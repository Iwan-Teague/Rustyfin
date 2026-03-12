use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::{Json, response::IntoResponse};
use rustfin_core::error::ApiError;
use rustfin_core::vault::{
    ConsumeVaultPairingCodeRequest, CreateVaultDeviceSessionRequest,
    CreateVaultDeviceSessionResponse, EncryptedVaultItem, UpsertVaultItemRequest,
    VaultAuditListResponse, VaultConfigResponse, VaultDestroyRequest, VaultDestroyResponse,
    VaultExportRequest, VaultExportResponse, VaultImportBitwardenRequest,
    VaultImportBitwardenResponse, VaultItemListResponse, VaultLookupRequest, VaultLookupResponse,
    VaultProtectedActionChallengeRequest, VaultProtectedActionChallengeResponse,
    VaultProtectedActionCompleteRequest, VaultProtectedActionCompleteResponse,
    VaultRevokeOtherSessionsRequest, VaultRevokeOtherSessionsResponse, VaultSessionRefreshRequest,
    VaultSyncResponse, VaultWrappedKeyMetadata,
};

use crate::auth::{AuthUser, VaultSessionUser};
use crate::error::AppError;
use crate::state::AppState;
use crate::vault::{audit, device_sessions, service};

#[derive(Debug, Default, serde::Deserialize)]
pub struct VaultItemListQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Default, serde::Deserialize)]
pub struct VaultSyncQuery {
    pub cursor: Option<i64>,
}

fn no_store_json<T: serde::Serialize>(value: T) -> impl IntoResponse {
    ([(header::CACHE_CONTROL, "no-store")], Json(value))
}

fn no_store_empty(status: StatusCode) -> impl IntoResponse {
    (status, [(header::CACHE_CONTROL, "no-store")])
}

fn touch_session_best_effort(state: &AppState, vault_session: &VaultSessionUser) {
    let db = state.db.clone();
    let session_id = vault_session.session_id.clone();
    tokio::spawn(async move {
        let _ = rustfin_db::repo::vault::touch_device_session(&db, &session_id, service::now_ts())
            .await;
    });
}

fn ensure_supported_wrapped_key(metadata: &VaultWrappedKeyMetadata) -> Result<(), AppError> {
    if metadata.kdf_algorithm != service::VAULT_KDF_ALGORITHM {
        return Err(ApiError::BadRequest("unsupported vault KDF algorithm".into()).into());
    }
    if metadata.hkdf_algorithm != service::VAULT_HKDF_ALGORITHM {
        return Err(ApiError::BadRequest("unsupported vault HKDF algorithm".into()).into());
    }
    if metadata.wrap_algorithm != service::VAULT_WRAP_ALGORITHM {
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
    let exists = rustfin_db::repo::vault::get_item(&state.db, user_id, target_item_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    if exists.is_none() {
        return Err(ApiError::NotFound("vault item not found".into()).into());
    }
    Ok(())
}

pub async fn get_config(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    let user_id = service::resolve_vault_owner_user_id(&state, &headers).await?;
    let wrapped_key = rustfin_db::repo::vault::get_active_wrapped_key(&state.db, &user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let item_count = rustfin_db::repo::vault::count_items(&state.db, &user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let response: VaultConfigResponse =
        service::build_vault_config_response(wrapped_key, item_count);
    Ok(no_store_json(response))
}

pub async fn bootstrap_vault(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<rustfin_core::vault::VaultBootstrapRequest>,
) -> Result<impl IntoResponse, AppError> {
    ensure_supported_wrapped_key(&body.wrapped_key)?;
    if rustfin_db::repo::vault::get_vault_account(&state.db, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .is_some()
    {
        return Err(ApiError::Conflict("vault already exists".into()).into());
    }

    let wrapped_key = rustfin_db::repo::vault::VaultWrappedKeyInsert {
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
    rustfin_db::repo::vault::bootstrap_vault(
        &state.db,
        &auth.user_id,
        "active",
        service::VAULT_SCHEMA_VERSION,
        wrapped_key.key_version,
        &wrapped_key,
        service::now_ts(),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    audit::record_event(
        &state,
        &auth.user_id,
        None,
        "vault_bootstrapped",
        None,
        serde_json::json!({}),
    )
    .await?;
    let wrapped_key = rustfin_db::repo::vault::get_active_wrapped_key(&state.db, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let item_count = rustfin_db::repo::vault::count_items(&state.db, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    Ok(no_store_json(service::build_vault_config_response(
        wrapped_key,
        item_count,
    )))
}

pub async fn rekey_vault(
    State(state): State<AppState>,
    auth: AuthUser,
    vault_session: VaultSessionUser,
    headers: HeaderMap,
    Json(body): Json<rustfin_core::vault::VaultBootstrapRequest>,
) -> Result<impl IntoResponse, AppError> {
    ensure_supported_wrapped_key(&body.wrapped_key)?;
    service::consume_protected_action_token(
        &state,
        &auth.user_id,
        Some(vault_session.session_id.as_str()),
        rustfin_core::vault::VaultProtectedActionKind::Rekey,
        None,
        headers
            .get("x-rustfin-vault-protected-action")
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| {
                ApiError::BadRequest("missing x-rustfin-vault-protected-action header".into())
            })?,
    )
    .await?;
    touch_session_best_effort(&state, &vault_session);

    let wrapped_key = rustfin_db::repo::vault::VaultWrappedKeyInsert {
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
    rustfin_db::repo::vault::rekey_vault(
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
        Some(&vault_session.session_id),
        "vault_rekeyed",
        None,
        serde_json::json!({ "key_version": wrapped_key.key_version }),
    )
    .await?;
    let active = rustfin_db::repo::vault::get_active_wrapped_key(&state.db, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let item_count = rustfin_db::repo::vault::count_items(&state.db, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    Ok(no_store_json(service::build_vault_config_response(
        active, item_count,
    )))
}

pub async fn list_items(
    State(state): State<AppState>,
    vault_session: VaultSessionUser,
    Query(query): Query<VaultItemListQuery>,
) -> Result<impl IntoResponse, AppError> {
    let limit = service::sanitize_limit(query.limit, 50);
    let offset = service::sanitize_offset(query.offset);
    let items = rustfin_db::repo::vault::list_item_summaries(
        &state.db,
        &vault_session.user_id,
        limit,
        offset,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let total = rustfin_db::repo::vault::count_items(&state.db, &vault_session.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let next_offset = if offset + limit < total {
        Some(offset + limit)
    } else {
        None
    };
    touch_session_best_effort(&state, &vault_session);
    Ok(no_store_json(VaultItemListResponse {
        items: items.into_iter().map(service::map_item_summary).collect(),
        next_offset,
        total,
    }))
}

pub async fn get_item(
    State(state): State<AppState>,
    vault_session: VaultSessionUser,
    Path(item_id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let item = rustfin_db::repo::vault::get_item(&state.db, &vault_session.user_id, &item_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("vault item not found".into()))?;
    touch_session_best_effort(&state, &vault_session);
    Ok(no_store_json(service::map_item(item)))
}

fn decode_upsert_item(
    body: UpsertVaultItemRequest,
) -> Result<rustfin_db::repo::vault::VaultItemUpsert, AppError> {
    if body.id.trim().is_empty() {
        return Err(ApiError::BadRequest("vault item id is required".into()).into());
    }
    if body.item_type.trim().is_empty() {
        return Err(ApiError::BadRequest("vault item_type is required".into()).into());
    }
    if body.uri_indexes.len() > service::VAULT_MAX_URI_INDEXES_PER_ITEM {
        return Err(ApiError::BadRequest("vault item has too many URI indexes".into()).into());
    }
    Ok(rustfin_db::repo::vault::VaultItemUpsert {
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
                Ok(rustfin_db::repo::vault::VaultUriIndexInput {
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
    vault_session: VaultSessionUser,
    Json(body): Json<UpsertVaultItemRequest>,
) -> Result<impl IntoResponse, AppError> {
    let item = decode_upsert_item(body)?;
    rustfin_db::repo::vault::upsert_item(&state.db, &vault_session.user_id, &item)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    audit::record_event(
        &state,
        &vault_session.user_id,
        Some(&vault_session.session_id),
        "vault_item_created",
        Some(&item.id),
        serde_json::json!({ "item_type": item.item_type }),
    )
    .await?;
    touch_session_best_effort(&state, &vault_session);
    Ok(no_store_empty(StatusCode::CREATED))
}

pub async fn replace_item(
    State(state): State<AppState>,
    vault_session: VaultSessionUser,
    Path(item_id): Path<String>,
    Json(body): Json<UpsertVaultItemRequest>,
) -> Result<impl IntoResponse, AppError> {
    if body.id != item_id {
        return Err(ApiError::BadRequest("item id path/body mismatch".into()).into());
    }
    if rustfin_db::repo::vault::get_item(&state.db, &vault_session.user_id, &item_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .is_none()
    {
        return Err(ApiError::NotFound("vault item not found".into()).into());
    }
    let item = decode_upsert_item(body)?;
    rustfin_db::repo::vault::upsert_item(&state.db, &vault_session.user_id, &item)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    audit::record_event(
        &state,
        &vault_session.user_id,
        Some(&vault_session.session_id),
        "vault_item_updated",
        Some(&item.id),
        serde_json::json!({ "item_type": item.item_type }),
    )
    .await?;
    touch_session_best_effort(&state, &vault_session);
    Ok(no_store_empty(StatusCode::NO_CONTENT))
}

pub async fn delete_item(
    State(state): State<AppState>,
    vault_session: VaultSessionUser,
    Path(item_id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let deleted = rustfin_db::repo::vault::soft_delete_item(
        &state.db,
        &vault_session.user_id,
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
        &vault_session.user_id,
        Some(&vault_session.session_id),
        "vault_item_deleted",
        Some(&item_id),
        serde_json::json!({}),
    )
    .await?;
    touch_session_best_effort(&state, &vault_session);
    Ok(no_store_empty(StatusCode::NO_CONTENT))
}

pub async fn lookup_items(
    State(state): State<AppState>,
    vault_session: VaultSessionUser,
    Json(body): Json<VaultLookupRequest>,
) -> Result<impl IntoResponse, AppError> {
    if body.match_hashes_hex.is_empty() {
        return Ok(no_store_json(VaultLookupResponse { items: Vec::new() }));
    }
    if body.match_hashes_hex.len() > service::VAULT_MAX_MATCH_HASHES {
        return Err(ApiError::BadRequest("too many vault lookup hashes".into()).into());
    }
    let match_hashes = body
        .match_hashes_hex
        .into_iter()
        .map(|value| service::decode_hex_field("match_hashes_hex", &value))
        .collect::<Result<Vec<_>, AppError>>()?;
    let items = rustfin_db::repo::vault::lookup_item_summaries(
        &state.db,
        &vault_session.user_id,
        &match_hashes,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    touch_session_best_effort(&state, &vault_session);
    Ok(no_store_json(VaultLookupResponse {
        items: items.into_iter().map(service::map_item_summary).collect(),
    }))
}

pub async fn sync_items(
    State(state): State<AppState>,
    vault_session: VaultSessionUser,
    Query(query): Query<VaultSyncQuery>,
) -> Result<impl IntoResponse, AppError> {
    let cursor = query.cursor.unwrap_or(0).max(0);
    let rows = rustfin_db::repo::vault::list_item_summaries_since(
        &state.db,
        &vault_session.user_id,
        cursor,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let next_cursor = rows
        .iter()
        .map(|row| row.updated_ts)
        .max()
        .unwrap_or(cursor);
    touch_session_best_effort(&state, &vault_session);
    Ok(no_store_json(VaultSyncResponse {
        cursor: next_cursor,
        items: rows.into_iter().map(service::map_item_summary).collect(),
    }))
}

pub async fn create_device_session(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    Json(body): Json<CreateVaultDeviceSessionRequest>,
) -> Result<impl IntoResponse, AppError> {
    let response: CreateVaultDeviceSessionResponse =
        device_sessions::create_device_session(&state, &auth.user_id, &headers, body).await?;
    Ok(no_store_json(response))
}

pub async fn consume_pairing_code(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ConsumeVaultPairingCodeRequest>,
) -> Result<impl IntoResponse, AppError> {
    let response = device_sessions::consume_pairing_code(&state, &headers, body).await?;
    Ok(no_store_json(response))
}

pub async fn refresh_device_session(
    State(state): State<AppState>,
    Json(body): Json<VaultSessionRefreshRequest>,
) -> Result<impl IntoResponse, AppError> {
    let response = device_sessions::refresh_device_session(&state, body).await?;
    Ok(no_store_json(response))
}

pub async fn list_device_sessions(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    let current_session = service::resolve_optional_vault_session(&state, &headers).await?;
    let rows = rustfin_db::repo::vault::list_device_sessions(&state.db, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let sessions = rows
        .into_iter()
        .map(|row| {
            service::map_device_session_response(
                row,
                current_session.as_ref().map(|ctx| ctx.session_id.as_str()),
            )
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    Ok(no_store_json(sessions))
}

pub async fn revoke_device_session(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(session_id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let revoked = rustfin_db::repo::vault::revoke_device_session(
        &state.db,
        &auth.user_id,
        &session_id,
        service::now_ts(),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    if !revoked {
        return Err(ApiError::NotFound("vault device session not found".into()).into());
    }
    audit::record_event(
        &state,
        &auth.user_id,
        None,
        "vault_device_session_revoked",
        None,
        serde_json::json!({ "session_id": session_id }),
    )
    .await?;
    Ok(no_store_empty(StatusCode::NO_CONTENT))
}

pub async fn revoke_other_device_sessions(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    Json(body): Json<VaultRevokeOtherSessionsRequest>,
) -> Result<impl IntoResponse, AppError> {
    let current_session = service::resolve_optional_vault_session(&state, &headers).await?;
    service::consume_protected_action_token(
        &state,
        &auth.user_id,
        current_session.as_ref().map(|ctx| ctx.session_id.as_str()),
        rustfin_core::vault::VaultProtectedActionKind::RevokeOtherSessions,
        None,
        &body.protected_action_token,
    )
    .await?;

    let revoked_count = rustfin_db::repo::vault::revoke_other_device_sessions(
        &state.db,
        &auth.user_id,
        current_session.as_ref().map(|ctx| ctx.session_id.as_str()),
        service::now_ts(),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    audit::record_event(
        &state,
        &auth.user_id,
        current_session.as_ref().map(|ctx| ctx.session_id.as_str()),
        "vault_other_device_sessions_revoked",
        None,
        serde_json::json!({ "revoked_count": revoked_count }),
    )
    .await?;
    Ok(no_store_json(VaultRevokeOtherSessionsResponse {
        revoked_count,
    }))
}

pub async fn challenge_protected_action(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    Json(body): Json<VaultProtectedActionChallengeRequest>,
) -> Result<impl IntoResponse, AppError> {
    ensure_target_item_owned(&state, &auth.user_id, body.target_item_id.as_deref()).await?;
    let user = rustfin_db::repo::users::find_by_id(&state.db, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::Unauthorized("user not found".into()))?;
    let valid =
        rustfin_db::repo::users::verify_password(&body.current_password, &user.password_hash)
            .map_err(|e| ApiError::Internal(format!("hash error: {e}")))?;
    if !valid {
        return Err(ApiError::Unauthorized("current password is incorrect".into()).into());
    }
    let session = service::resolve_optional_vault_session(&state, &headers).await?;
    let action_token = service::generate_secret_token(64);
    let token_hash = service::hash_secret(&action_token);
    let expires_ts = service::now_ts() + service::VAULT_PROTECTED_ACTION_TTL_SECONDS;
    rustfin_db::repo::vault::create_protected_action_token(
        &state.db,
        &rustfin_db::repo::vault::CreateProtectedActionTokenInput {
            id: uuid::Uuid::new_v4().to_string(),
            user_id: auth.user_id.clone(),
            device_session_id: session.as_ref().map(|ctx| ctx.session_id.clone()),
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
        session.as_ref().map(|ctx| ctx.session_id.as_str()),
        "vault_protected_action_challenged",
        body.target_item_id.as_deref(),
        serde_json::json!({ "action_kind": body.action_kind.as_str() }),
    )
    .await?;
    Ok(no_store_json(VaultProtectedActionChallengeResponse {
        action_token,
        action_kind: body.action_kind,
        expires_ts,
    }))
}

pub async fn complete_protected_action(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    Json(body): Json<VaultProtectedActionCompleteRequest>,
) -> Result<impl IntoResponse, AppError> {
    let session = service::resolve_optional_vault_session(&state, &headers).await?;
    ensure_target_item_owned(&state, &auth.user_id, body.target_item_id.as_deref()).await?;
    service::consume_protected_action_token(
        &state,
        &auth.user_id,
        session.as_ref().map(|ctx| ctx.session_id.as_str()),
        body.action_kind,
        body.target_item_id.as_deref(),
        &body.action_token,
    )
    .await?;
    Ok(no_store_json(VaultProtectedActionCompleteResponse {
        ok: true,
    }))
}

pub async fn list_audit_events(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<impl IntoResponse, AppError> {
    let rows = rustfin_db::repo::vault::list_audit_events(
        &state.db,
        &auth.user_id,
        service::VAULT_AUDIT_LIMIT,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    Ok(no_store_json(VaultAuditListResponse {
        events: rows
            .into_iter()
            .map(|row| rustfin_core::vault::VaultAuditEventResponse {
                id: row.id,
                event_kind: row.event_kind,
                target_item_id: row.target_item_id,
                created_ts: row.created_ts,
                event_json: row.event_json,
            })
            .collect(),
    }))
}

pub async fn export_vault(
    State(state): State<AppState>,
    auth: AuthUser,
    vault_session: VaultSessionUser,
    Json(body): Json<VaultExportRequest>,
) -> Result<impl IntoResponse, AppError> {
    service::consume_protected_action_token(
        &state,
        &auth.user_id,
        Some(vault_session.session_id.as_str()),
        rustfin_core::vault::VaultProtectedActionKind::Export,
        None,
        &body.protected_action_token,
    )
    .await?;
    touch_session_best_effort(&state, &vault_session);
    let wrapped_key = rustfin_db::repo::vault::get_active_wrapped_key(&state.db, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let item_count = rustfin_db::repo::vault::count_items(&state.db, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let items: Vec<EncryptedVaultItem> =
        rustfin_db::repo::vault::list_all_items(&state.db, &auth.user_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .into_iter()
            .map(service::map_item)
            .collect();
    audit::record_event(
        &state,
        &auth.user_id,
        Some(&vault_session.session_id),
        "vault_export_requested",
        None,
        serde_json::json!({ "item_count": items.len() }),
    )
    .await?;
    Ok(no_store_json(VaultExportResponse {
        config: service::build_vault_config_response(wrapped_key, item_count),
        items,
    }))
}

pub async fn import_bitwarden(
    State(state): State<AppState>,
    auth: AuthUser,
    vault_session: VaultSessionUser,
    Json(body): Json<VaultImportBitwardenRequest>,
) -> Result<impl IntoResponse, AppError> {
    if body.items.len() > service::VAULT_MAX_IMPORT_ITEMS {
        return Err(ApiError::BadRequest("import exceeds vault item limit".into()).into());
    }
    service::consume_protected_action_token(
        &state,
        &auth.user_id,
        Some(vault_session.session_id.as_str()),
        rustfin_core::vault::VaultProtectedActionKind::ImportOverwrite,
        None,
        &body.protected_action_token,
    )
    .await?;
    touch_session_best_effort(&state, &vault_session);

    if body.clear_existing {
        rustfin_db::repo::vault::clear_all_items(&state.db, &auth.user_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    }

    let mut imported_count = 0usize;
    for item in body.items {
        let decoded = decode_upsert_item(item)?;
        rustfin_db::repo::vault::upsert_item(&state.db, &auth.user_id, &decoded)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
        imported_count += 1;
    }
    audit::record_event(
        &state,
        &auth.user_id,
        Some(&vault_session.session_id),
        "vault_import_completed",
        None,
        serde_json::json!({ "imported_count": imported_count, "cleared_existing": body.clear_existing }),
    )
    .await?;
    Ok(no_store_json(VaultImportBitwardenResponse {
        imported_count,
        cleared_existing: body.clear_existing,
    }))
}

pub async fn destroy_vault(
    State(state): State<AppState>,
    auth: AuthUser,
    vault_session: VaultSessionUser,
    Json(body): Json<VaultDestroyRequest>,
) -> Result<impl IntoResponse, AppError> {
    service::consume_protected_action_token(
        &state,
        &auth.user_id,
        Some(vault_session.session_id.as_str()),
        rustfin_core::vault::VaultProtectedActionKind::DestroyVault,
        None,
        &body.protected_action_token,
    )
    .await?;
    rustfin_db::repo::vault::destroy_vault(&state.db, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    Ok(no_store_json(VaultDestroyResponse { destroyed: true }))
}
