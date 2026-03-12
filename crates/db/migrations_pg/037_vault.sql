CREATE TABLE IF NOT EXISTS vault_account (
    user_id TEXT PRIMARY KEY REFERENCES "user"(id) ON DELETE CASCADE,
    status TEXT NOT NULL,
    schema_version INTEGER NOT NULL,
    active_key_version INTEGER NOT NULL,
    created_ts BIGINT NOT NULL,
    updated_ts BIGINT NOT NULL,
    last_unlock_required_ts BIGINT,
    last_rekey_ts BIGINT
);

CREATE TABLE IF NOT EXISTS vault_wrapped_key (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
    key_version INTEGER NOT NULL,
    kdf_algorithm TEXT NOT NULL,
    kdf_memory_kib INTEGER NOT NULL,
    kdf_iterations INTEGER NOT NULL,
    kdf_parallelism INTEGER NOT NULL,
    kdf_salt BYTEA NOT NULL,
    hkdf_algorithm TEXT NOT NULL,
    wrap_algorithm TEXT NOT NULL,
    wrap_nonce BYTEA NOT NULL,
    wrapped_vault_key BYTEA NOT NULL,
    created_ts BIGINT NOT NULL,
    superseded_ts BIGINT,
    UNIQUE (user_id, key_version)
);

CREATE TABLE IF NOT EXISTS vault_item (
    user_id TEXT NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
    id TEXT NOT NULL,
    item_type TEXT NOT NULL,
    key_version INTEGER NOT NULL,
    summary_ciphertext BYTEA NOT NULL,
    summary_nonce BYTEA NOT NULL,
    summary_version INTEGER NOT NULL,
    payload_ciphertext BYTEA NOT NULL,
    payload_nonce BYTEA NOT NULL,
    payload_version INTEGER NOT NULL,
    favorite BOOLEAN NOT NULL DEFAULT FALSE,
    revision BIGINT NOT NULL,
    deleted_ts BIGINT,
    created_ts BIGINT NOT NULL,
    updated_ts BIGINT NOT NULL,
    PRIMARY KEY (user_id, id)
);

CREATE TABLE IF NOT EXISTS vault_item_uri_index (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
    item_id TEXT NOT NULL,
    match_hash BYTEA NOT NULL,
    match_type TEXT NOT NULL,
    rank INTEGER NOT NULL DEFAULT 0,
    created_ts BIGINT NOT NULL,
    CONSTRAINT vault_item_uri_index_item_fk
        FOREIGN KEY (user_id, item_id)
        REFERENCES vault_item(user_id, id)
        ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS vault_device_session (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
    client_kind TEXT NOT NULL,
    device_name TEXT NOT NULL,
    device_platform TEXT,
    device_fingerprint_hash TEXT,
    refresh_token_family_id TEXT NOT NULL,
    refresh_token_hash TEXT NOT NULL UNIQUE,
    created_ts BIGINT NOT NULL,
    last_used_ts BIGINT NOT NULL,
    expires_ts BIGINT NOT NULL,
    revoked_ts BIGINT,
    ip_summary TEXT,
    user_agent_summary TEXT
);

CREATE TABLE IF NOT EXISTS vault_pending_device_approval (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
    client_kind TEXT NOT NULL,
    device_name TEXT NOT NULL,
    fingerprint_phrase TEXT NOT NULL,
    pairing_code_hash TEXT NOT NULL UNIQUE,
    created_ts BIGINT NOT NULL,
    expires_ts BIGINT NOT NULL,
    approved_ts BIGINT,
    denied_ts BIGINT
);

CREATE TABLE IF NOT EXISTS vault_protected_action_token (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
    device_session_id TEXT,
    action_kind TEXT NOT NULL,
    target_item_id TEXT,
    token_hash TEXT NOT NULL UNIQUE,
    created_ts BIGINT NOT NULL,
    expires_ts BIGINT NOT NULL,
    consumed_ts BIGINT
);

CREATE TABLE IF NOT EXISTS vault_audit_event (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
    device_session_id TEXT,
    event_kind TEXT NOT NULL,
    target_item_id TEXT,
    event_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_ts BIGINT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_vault_wrapped_key_user_active
    ON vault_wrapped_key(user_id, superseded_ts, key_version DESC);

CREATE INDEX IF NOT EXISTS idx_vault_item_user_updated
    ON vault_item(user_id, updated_ts DESC, id);

CREATE INDEX IF NOT EXISTS idx_vault_item_user_deleted
    ON vault_item(user_id, deleted_ts, updated_ts DESC);

CREATE INDEX IF NOT EXISTS idx_vault_item_uri_index_lookup
    ON vault_item_uri_index(user_id, match_hash);

CREATE INDEX IF NOT EXISTS idx_vault_device_session_user_last_used
    ON vault_device_session(user_id, last_used_ts DESC);

CREATE INDEX IF NOT EXISTS idx_vault_device_session_refresh_family
    ON vault_device_session(user_id, refresh_token_family_id);

CREATE INDEX IF NOT EXISTS idx_vault_pending_device_approval_user_created
    ON vault_pending_device_approval(user_id, created_ts DESC);

CREATE INDEX IF NOT EXISTS idx_vault_protected_action_user_kind
    ON vault_protected_action_token(user_id, action_kind, expires_ts DESC);

CREATE INDEX IF NOT EXISTS idx_vault_audit_event_user_created
    ON vault_audit_event(user_id, created_ts DESC);
