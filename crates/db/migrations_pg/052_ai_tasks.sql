CREATE TABLE IF NOT EXISTS ai_task (
    id UUID PRIMARY KEY,
    owner_user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    task_type TEXT NOT NULL,
    status TEXT NOT NULL,
    requested_model TEXT NULL,
    effective_answer_model TEXT NULL,
    effective_planner_model TEXT NULL,
    input_json JSONB NOT NULL,
    result_json JSONB NULL,
    error_json JSONB NULL,
    progress_pct DOUBLE PRECISION NOT NULL DEFAULT 0,
    phase TEXT NOT NULL,
    cancel_requested BOOLEAN NOT NULL DEFAULT FALSE,
    checkpoint_version INTEGER NOT NULL DEFAULT 0,
    last_checkpoint_json JSONB NULL
);

CREATE INDEX IF NOT EXISTS idx_ai_task_owner_created_at
    ON ai_task(owner_user_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_ai_task_status_updated_at
    ON ai_task(status, updated_at DESC);

CREATE TABLE IF NOT EXISTS ai_task_event (
    id BIGSERIAL PRIMARY KEY,
    task_id UUID NOT NULL REFERENCES ai_task(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    event_type TEXT NOT NULL,
    payload_json JSONB NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_ai_task_event_task_created_at
    ON ai_task_event(task_id, id ASC);

CREATE TABLE IF NOT EXISTS ai_task_checkpoint (
    id BIGSERIAL PRIMARY KEY,
    task_id UUID NOT NULL REFERENCES ai_task(id) ON DELETE CASCADE,
    version INTEGER NOT NULL,
    phase TEXT NOT NULL,
    payload_json JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_ai_task_checkpoint_task_version
    ON ai_task_checkpoint(task_id, version DESC);

CREATE TABLE IF NOT EXISTS ai_task_artifact (
    id UUID PRIMARY KEY,
    task_id UUID NOT NULL REFERENCES ai_task(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    file_name TEXT NOT NULL,
    media_type TEXT NOT NULL,
    storage_path TEXT NOT NULL,
    size_bytes BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_ai_task_artifact_task_created_at
    ON ai_task_artifact(task_id, created_at DESC);
