-- Settings table: key-value store for server-wide configuration
CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Insert defaults for setup state
INSERT INTO settings (key, value) VALUES ('setup_completed', 'false') ON CONFLICT DO NOTHING;
INSERT INTO settings (key, value) VALUES ('setup_state', 'NotStarted') ON CONFLICT DO NOTHING;
INSERT INTO settings (key, value) VALUES ('server_name', 'Rustyfin') ON CONFLICT DO NOTHING;
INSERT INTO settings (key, value) VALUES ('default_ui_locale', 'en') ON CONFLICT DO NOTHING;
INSERT INTO settings (key, value) VALUES ('default_region', 'US') ON CONFLICT DO NOTHING;
INSERT INTO settings (key, value) VALUES ('default_time_zone', '') ON CONFLICT DO NOTHING;
INSERT INTO settings (key, value) VALUES ('metadata_language', 'en') ON CONFLICT DO NOTHING;
INSERT INTO settings (key, value) VALUES ('metadata_region', 'US') ON CONFLICT DO NOTHING;
INSERT INTO settings (key, value) VALUES ('allow_remote_access', 'false') ON CONFLICT DO NOTHING;
INSERT INTO settings (key, value) VALUES ('enable_automatic_port_mapping', 'false') ON CONFLICT DO NOTHING;
INSERT INTO settings (key, value) VALUES ('trusted_proxies', '[]') ON CONFLICT DO NOTHING;

-- Setup session table: exclusive writer lock for setup wizard
CREATE TABLE IF NOT EXISTS setup_session (
    id           INTEGER PRIMARY KEY CHECK (id = 1),
    owner_token_hash TEXT NOT NULL,
    client_name  TEXT NOT NULL,
    claimed_at   BIGINT NOT NULL,
    expires_at   BIGINT NOT NULL
);

-- Idempotency keys table: safe retries for create-only operations
CREATE TABLE IF NOT EXISTS idempotency_keys (
    key          TEXT PRIMARY KEY,
    endpoint     TEXT NOT NULL,
    payload_hash TEXT NOT NULL,
    response     TEXT NOT NULL,
    status_code  INTEGER NOT NULL,
    created_at   BIGINT NOT NULL
);
