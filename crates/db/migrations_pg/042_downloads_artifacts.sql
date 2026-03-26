CREATE TABLE IF NOT EXISTS download_artifact (
    id TEXT PRIMARY KEY,
    artifact_id TEXT NOT NULL,
    title TEXT NOT NULL,
    summary TEXT NOT NULL,
    detail TEXT NOT NULL,
    platform TEXT NOT NULL,
    architecture TEXT NOT NULL,
    version TEXT,
    channel TEXT NOT NULL DEFAULT 'stable',
    filename TEXT,
    file_size BIGINT,
    checksum TEXT,
    signature_status TEXT NOT NULL DEFAULT 'unsigned',
    distribution_mode TEXT NOT NULL DEFAULT 'direct',
    external_url TEXT,
    availability TEXT NOT NULL DEFAULT 'planned',
    requires_sign_in BOOLEAN NOT NULL DEFAULT true,
    install_steps_json TEXT NOT NULL DEFAULT '[]',
    created_ts BIGINT NOT NULL,
    updated_ts BIGINT NOT NULL,
    UNIQUE(artifact_id, version, platform, architecture)
);

CREATE INDEX IF NOT EXISTS idx_download_artifact_availability ON download_artifact(availability);
