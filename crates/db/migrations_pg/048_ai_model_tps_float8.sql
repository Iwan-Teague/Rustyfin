ALTER TABLE ai_model_benchmark
    ALTER COLUMN tokens_per_second TYPE DOUBLE PRECISION
    USING tokens_per_second::DOUBLE PRECISION;

ALTER TABLE ai_model_profile
    ALTER COLUMN last_tokens_per_second TYPE DOUBLE PRECISION
    USING last_tokens_per_second::DOUBLE PRECISION;
