CREATE TABLE IF NOT EXISTS ai_assistant_audit_event (
    id TEXT PRIMARY KEY,
    trace_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    username TEXT NOT NULL,
    user_role TEXT NOT NULL,
    model_name TEXT NOT NULL,
    message_preview TEXT NOT NULL,
    history_len BIGINT NOT NULL,
    response_kind TEXT NOT NULL,
    planned_tools_json TEXT NOT NULL,
    executed_tools_json TEXT NOT NULL,
    grounding_sources_json TEXT NOT NULL,
    error_message TEXT,
    created_ts BIGINT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_ai_assistant_audit_event_trace_id
    ON ai_assistant_audit_event (trace_id);

CREATE INDEX IF NOT EXISTS idx_ai_assistant_audit_event_created_ts
    ON ai_assistant_audit_event (created_ts DESC);

CREATE INDEX IF NOT EXISTS idx_ai_assistant_audit_event_user_id_created_ts
    ON ai_assistant_audit_event (user_id, created_ts DESC);
