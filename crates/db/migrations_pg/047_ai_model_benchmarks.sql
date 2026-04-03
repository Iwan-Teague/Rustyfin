CREATE TABLE IF NOT EXISTS ai_model_benchmark (
    id TEXT PRIMARY KEY,
    host_fingerprint TEXT NOT NULL,
    model_name TEXT NOT NULL,
    model_checksum TEXT NOT NULL,
    model_path TEXT NOT NULL,
    benchmark_label TEXT NOT NULL,
    backend_kind TEXT NOT NULL,
    n_threads INTEGER NOT NULL,
    n_gpu_layers INTEGER NOT NULL,
    split_mode TEXT NOT NULL,
    main_gpu INTEGER,
    device_indices_json TEXT NOT NULL DEFAULT '[]',
    load_duration_ms BIGINT NOT NULL,
    prefill_tokens BIGINT NOT NULL,
    prefill_duration_ms BIGINT NOT NULL,
    decode_tokens BIGINT NOT NULL,
    decode_duration_ms BIGINT NOT NULL,
    first_token_ms BIGINT NOT NULL,
    total_duration_ms BIGINT NOT NULL,
    tokens_per_second REAL NOT NULL,
    rss_before_bytes BIGINT,
    rss_after_load_bytes BIGINT,
    rss_peak_bytes BIGINT,
    failure_message TEXT,
    notes_json TEXT NOT NULL DEFAULT '[]',
    created_ts BIGINT NOT NULL,
    updated_ts BIGINT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_ai_model_benchmark_unique_run
    ON ai_model_benchmark (
        host_fingerprint,
        model_checksum,
        benchmark_label,
        n_threads,
        n_gpu_layers,
        split_mode,
        COALESCE(main_gpu, -1)
    );

CREATE INDEX IF NOT EXISTS idx_ai_model_benchmark_host_updated
    ON ai_model_benchmark (host_fingerprint, updated_ts DESC);

CREATE INDEX IF NOT EXISTS idx_ai_model_benchmark_model_updated
    ON ai_model_benchmark (model_checksum, updated_ts DESC);

CREATE TABLE IF NOT EXISTS ai_model_profile (
    id TEXT PRIMARY KEY,
    host_fingerprint TEXT NOT NULL,
    model_name TEXT NOT NULL,
    model_checksum TEXT NOT NULL,
    model_path TEXT NOT NULL,
    context_window INTEGER NOT NULL,
    preferred_completion_tokens INTEGER NOT NULL,
    planner_max_output INTEGER NOT NULL,
    summary_max_output INTEGER NOT NULL,
    safety_headroom INTEGER NOT NULL,
    warmup_cost_class TEXT NOT NULL,
    supports_structured_output BOOLEAN NOT NULL,
    supports_prompt_cache BOOLEAN NOT NULL,
    recommended_n_threads INTEGER NOT NULL,
    recommended_n_gpu_layers INTEGER NOT NULL,
    recommended_split_mode TEXT NOT NULL,
    recommended_main_gpu INTEGER,
    recommended_device_indices_json TEXT NOT NULL DEFAULT '[]',
    estimated_model_bytes BIGINT NOT NULL,
    notes_json TEXT NOT NULL DEFAULT '[]',
    last_benchmark_label TEXT NOT NULL,
    last_load_duration_ms BIGINT NOT NULL,
    last_tokens_per_second REAL NOT NULL,
    benchmark_count BIGINT NOT NULL,
    created_ts BIGINT NOT NULL,
    updated_ts BIGINT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_ai_model_profile_unique_model
    ON ai_model_profile (host_fingerprint, model_checksum);

CREATE INDEX IF NOT EXISTS idx_ai_model_profile_host_updated
    ON ai_model_profile (host_fingerprint, updated_ts DESC);

CREATE INDEX IF NOT EXISTS idx_ai_model_profile_model_updated
    ON ai_model_profile (model_checksum, updated_ts DESC);
