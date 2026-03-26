CREATE TABLE IF NOT EXISTS backup_policy (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    schedule_cron TEXT,
    retention_count INTEGER NOT NULL DEFAULT 5,
    target_type TEXT NOT NULL DEFAULT 'local',
    target_path TEXT,
    include_database BOOLEAN NOT NULL DEFAULT true,
    include_server_config BOOLEAN NOT NULL DEFAULT true,
    include_server_worlds BOOLEAN NOT NULL DEFAULT true,
    enabled BOOLEAN NOT NULL DEFAULT true,
    last_run_ts BIGINT,
    created_ts BIGINT NOT NULL,
    updated_ts BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS backup_job (
    id TEXT PRIMARY KEY,
    policy_id TEXT REFERENCES backup_policy(id),
    status TEXT NOT NULL, -- pending, running, success, failed
    trigger_type TEXT NOT NULL, -- manual, scheduled
    start_ts BIGINT NOT NULL,
    end_ts BIGINT,
    log_text TEXT,
    error_message TEXT,
    total_size_bytes BIGINT
);

CREATE TABLE IF NOT EXISTS backup_artifact (
    id TEXT PRIMARY KEY,
    job_id TEXT NOT NULL REFERENCES backup_job(id) ON DELETE CASCADE,
    artifact_type TEXT NOT NULL, -- postgres_dump, config_archive, world_archive
    filename TEXT NOT NULL,
    file_path TEXT NOT NULL,
    file_size_bytes BIGINT NOT NULL,
    checksum_sha256 TEXT,
    created_ts BIGINT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_backup_job_start_ts ON backup_job(start_ts DESC);
CREATE INDEX IF NOT EXISTS idx_backup_artifact_job_id ON backup_artifact(job_id);
