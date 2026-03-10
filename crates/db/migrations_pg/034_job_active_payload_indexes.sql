CREATE INDEX IF NOT EXISTS idx_job_active_kind_payload
ON job (kind, status, payload_json)
WHERE status IN ('queued', 'running') AND payload_json IS NOT NULL;
