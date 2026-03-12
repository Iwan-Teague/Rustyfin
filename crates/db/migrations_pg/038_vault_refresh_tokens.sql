CREATE TABLE IF NOT EXISTS vault_device_session_refresh_token (
    id TEXT PRIMARY KEY,
    device_session_id TEXT NOT NULL REFERENCES vault_device_session(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
    refresh_token_family_id TEXT NOT NULL,
    token_hash TEXT NOT NULL UNIQUE,
    created_ts BIGINT NOT NULL,
    expires_ts BIGINT NOT NULL,
    consumed_ts BIGINT,
    revoked_ts BIGINT
);

CREATE INDEX IF NOT EXISTS idx_vault_refresh_token_session
    ON vault_device_session_refresh_token (device_session_id, expires_ts DESC);

CREATE INDEX IF NOT EXISTS idx_vault_refresh_token_user
    ON vault_device_session_refresh_token (user_id, expires_ts DESC);

CREATE INDEX IF NOT EXISTS idx_vault_refresh_token_family
    ON vault_device_session_refresh_token (refresh_token_family_id, expires_ts DESC);
