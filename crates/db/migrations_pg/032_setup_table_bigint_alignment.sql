ALTER TABLE setup_session
    ALTER COLUMN claimed_at TYPE BIGINT USING claimed_at::BIGINT,
    ALTER COLUMN expires_at TYPE BIGINT USING expires_at::BIGINT;

ALTER TABLE idempotency_keys
    ALTER COLUMN created_at TYPE BIGINT USING created_at::BIGINT;
