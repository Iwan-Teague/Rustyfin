-- Settings table: key-value store for server-wide configuration
CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Insert defaults for setup state
INSERT OR IGNORE INTO settings (key, value) VALUES ('setup_completed', 'false');
INSERT OR IGNORE INTO settings (key, value) VALUES ('setup_state', 'NotStarted');
INSERT OR IGNORE INTO settings (key, value) VALUES ('server_name', 'Rustyfin');
INSERT OR IGNORE INTO settings (key, value) VALUES ('default_ui_locale', 'en');
INSERT OR IGNORE INTO settings (key, value) VALUES ('default_region', 'US');
INSERT OR IGNORE INTO settings (key, value) VALUES ('default_time_zone', '');
INSERT OR IGNORE INTO settings (key, value) VALUES ('metadata_language', 'en');
INSERT OR IGNORE INTO settings (key, value) VALUES ('metadata_region', 'US');
INSERT OR IGNORE INTO settings (key, value) VALUES ('allow_remote_access', 'false');
INSERT OR IGNORE INTO settings (key, value) VALUES ('enable_automatic_port_mapping', 'false');
INSERT OR IGNORE INTO settings (key, value) VALUES ('trusted_proxies', '[]');

-- Setup session table: exclusive writer lock for setup wizard
CREATE TABLE IF NOT EXISTS setup_session (
    id           INTEGER PRIMARY KEY CHECK (id = 1),
    owner_token_hash TEXT NOT NULL,
    client_name  TEXT NOT NULL,
    claimed_at   INTEGER NOT NULL,
    expires_at   INTEGER NOT NULL
);

-- Idempotency keys table: safe retries for create-only operations
CREATE TABLE IF NOT EXISTS idempotency_keys (
    key          TEXT PRIMARY KEY,
    endpoint     TEXT NOT NULL,
    payload_hash TEXT NOT NULL,
    response     TEXT NOT NULL,
    status_code  INTEGER NOT NULL,
    created_at   INTEGER NOT NULL
);
