ALTER TABLE ai_assistant_audit_event
    ADD COLUMN IF NOT EXISTS planner_json TEXT NOT NULL DEFAULT '{}';

ALTER TABLE ai_assistant_audit_event
    ADD COLUMN IF NOT EXISTS model_routing_json TEXT NOT NULL DEFAULT '[]';
