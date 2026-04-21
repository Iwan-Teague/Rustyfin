use crate::DbPool;
use sqlx::Row;

#[derive(Debug, Clone)]
pub struct AiModelBenchmarkRow {
    pub id: String,
    pub host_fingerprint: String,
    pub model_name: String,
    pub model_checksum: String,
    pub model_path: String,
    pub benchmark_label: String,
    pub backend_kind: String,
    pub n_threads: i32,
    pub n_gpu_layers: i32,
    pub split_mode: String,
    pub main_gpu: Option<i32>,
    pub device_indices_json: String,
    pub load_duration_ms: i64,
    pub prefill_tokens: i64,
    pub prefill_duration_ms: i64,
    pub decode_tokens: i64,
    pub decode_duration_ms: i64,
    pub first_token_ms: i64,
    pub total_duration_ms: i64,
    pub tokens_per_second: f64,
    pub rss_before_bytes: Option<i64>,
    pub rss_after_load_bytes: Option<i64>,
    pub rss_peak_bytes: Option<i64>,
    pub failure_message: Option<String>,
    pub notes_json: String,
    pub created_ts: i64,
    pub updated_ts: i64,
}

pub struct UpsertAiModelBenchmarkParams<'a> {
    pub host_fingerprint: &'a str,
    pub model_name: &'a str,
    pub model_checksum: &'a str,
    pub model_path: &'a str,
    pub benchmark_label: &'a str,
    pub backend_kind: &'a str,
    pub n_threads: i32,
    pub n_gpu_layers: i32,
    pub split_mode: &'a str,
    pub main_gpu: Option<i32>,
    pub device_indices_json: &'a str,
    pub load_duration_ms: i64,
    pub prefill_tokens: i64,
    pub prefill_duration_ms: i64,
    pub decode_tokens: i64,
    pub decode_duration_ms: i64,
    pub first_token_ms: i64,
    pub total_duration_ms: i64,
    pub tokens_per_second: f64,
    pub rss_before_bytes: Option<i64>,
    pub rss_after_load_bytes: Option<i64>,
    pub rss_peak_bytes: Option<i64>,
    pub failure_message: Option<&'a str>,
    pub notes_json: &'a str,
}

#[derive(Debug, Clone)]
pub struct AiModelProfileRow {
    pub id: String,
    pub host_fingerprint: String,
    pub model_name: String,
    pub model_checksum: String,
    pub model_path: String,
    pub context_window: i32,
    pub preferred_completion_tokens: i32,
    pub planner_max_output: i32,
    pub summary_max_output: i32,
    pub safety_headroom: i32,
    pub warmup_cost_class: String,
    pub supports_structured_output: bool,
    pub supports_prompt_cache: bool,
    pub recommended_n_threads: i32,
    pub recommended_n_gpu_layers: i32,
    pub recommended_split_mode: String,
    pub recommended_main_gpu: Option<i32>,
    pub recommended_device_indices_json: String,
    pub estimated_model_bytes: i64,
    pub notes_json: String,
    pub last_benchmark_label: String,
    pub last_load_duration_ms: i64,
    pub last_tokens_per_second: f64,
    pub benchmark_count: i64,
    pub created_ts: i64,
    pub updated_ts: i64,
}

fn map_benchmark_row(row: &sqlx::postgres::PgRow) -> Result<AiModelBenchmarkRow, sqlx::Error> {
    Ok(AiModelBenchmarkRow {
        id: row.try_get("id")?,
        host_fingerprint: row.try_get("host_fingerprint")?,
        model_name: row.try_get("model_name")?,
        model_checksum: row.try_get("model_checksum")?,
        model_path: row.try_get("model_path")?,
        benchmark_label: row.try_get("benchmark_label")?,
        backend_kind: row.try_get("backend_kind")?,
        n_threads: row.try_get("n_threads")?,
        n_gpu_layers: row.try_get("n_gpu_layers")?,
        split_mode: row.try_get("split_mode")?,
        main_gpu: row.try_get("main_gpu")?,
        device_indices_json: row.try_get("device_indices_json")?,
        load_duration_ms: row.try_get("load_duration_ms")?,
        prefill_tokens: row.try_get("prefill_tokens")?,
        prefill_duration_ms: row.try_get("prefill_duration_ms")?,
        decode_tokens: row.try_get("decode_tokens")?,
        decode_duration_ms: row.try_get("decode_duration_ms")?,
        first_token_ms: row.try_get("first_token_ms")?,
        total_duration_ms: row.try_get("total_duration_ms")?,
        tokens_per_second: row.try_get("tokens_per_second")?,
        rss_before_bytes: row.try_get("rss_before_bytes")?,
        rss_after_load_bytes: row.try_get("rss_after_load_bytes")?,
        rss_peak_bytes: row.try_get("rss_peak_bytes")?,
        failure_message: row.try_get("failure_message")?,
        notes_json: row.try_get("notes_json")?,
        created_ts: row.try_get("created_ts")?,
        updated_ts: row.try_get("updated_ts")?,
    })
}

fn map_profile_row(row: &sqlx::postgres::PgRow) -> Result<AiModelProfileRow, sqlx::Error> {
    Ok(AiModelProfileRow {
        id: row.try_get("id")?,
        host_fingerprint: row.try_get("host_fingerprint")?,
        model_name: row.try_get("model_name")?,
        model_checksum: row.try_get("model_checksum")?,
        model_path: row.try_get("model_path")?,
        context_window: row.try_get("context_window")?,
        preferred_completion_tokens: row.try_get("preferred_completion_tokens")?,
        planner_max_output: row.try_get("planner_max_output")?,
        summary_max_output: row.try_get("summary_max_output")?,
        safety_headroom: row.try_get("safety_headroom")?,
        warmup_cost_class: row.try_get("warmup_cost_class")?,
        supports_structured_output: row.try_get("supports_structured_output")?,
        supports_prompt_cache: row.try_get("supports_prompt_cache")?,
        recommended_n_threads: row.try_get("recommended_n_threads")?,
        recommended_n_gpu_layers: row.try_get("recommended_n_gpu_layers")?,
        recommended_split_mode: row.try_get("recommended_split_mode")?,
        recommended_main_gpu: row.try_get("recommended_main_gpu")?,
        recommended_device_indices_json: row.try_get("recommended_device_indices_json")?,
        estimated_model_bytes: row.try_get("estimated_model_bytes")?,
        notes_json: row.try_get("notes_json")?,
        last_benchmark_label: row.try_get("last_benchmark_label")?,
        last_load_duration_ms: row.try_get("last_load_duration_ms")?,
        last_tokens_per_second: row.try_get("last_tokens_per_second")?,
        benchmark_count: row.try_get("benchmark_count")?,
        created_ts: row.try_get("created_ts")?,
        updated_ts: row.try_get("updated_ts")?,
    })
}

pub struct UpsertAiModelProfileParams<'a> {
    pub host_fingerprint: &'a str,
    pub model_name: &'a str,
    pub model_checksum: &'a str,
    pub model_path: &'a str,
    pub context_window: i32,
    pub preferred_completion_tokens: i32,
    pub planner_max_output: i32,
    pub summary_max_output: i32,
    pub safety_headroom: i32,
    pub warmup_cost_class: &'a str,
    pub supports_structured_output: bool,
    pub supports_prompt_cache: bool,
    pub recommended_n_threads: i32,
    pub recommended_n_gpu_layers: i32,
    pub recommended_split_mode: &'a str,
    pub recommended_main_gpu: Option<i32>,
    pub recommended_device_indices_json: &'a str,
    pub estimated_model_bytes: i64,
    pub notes_json: &'a str,
    pub last_benchmark_label: &'a str,
    pub last_load_duration_ms: i64,
    pub last_tokens_per_second: f64,
    pub benchmark_count: i64,
}

async fn upsert_model_benchmark_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    params: UpsertAiModelBenchmarkParams<'_>,
) -> Result<AiModelBenchmarkRow, sqlx::Error> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let row = sqlx::query(
        "INSERT INTO ai_model_benchmark (
            id, host_fingerprint, model_name, model_checksum, model_path, benchmark_label,
            backend_kind, n_threads, n_gpu_layers, split_mode, main_gpu, device_indices_json,
            load_duration_ms, prefill_tokens, prefill_duration_ms, decode_tokens, decode_duration_ms,
            first_token_ms, total_duration_ms, tokens_per_second, rss_before_bytes, rss_after_load_bytes,
            rss_peak_bytes, failure_message, notes_json, created_ts, updated_ts
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,$25,$26,$27)
        ON CONFLICT (host_fingerprint, model_checksum, benchmark_label, n_threads, n_gpu_layers, split_mode, COALESCE(main_gpu, -1))
        DO UPDATE SET
            model_name = EXCLUDED.model_name,
            model_path = EXCLUDED.model_path,
            backend_kind = EXCLUDED.backend_kind,
            device_indices_json = EXCLUDED.device_indices_json,
            load_duration_ms = EXCLUDED.load_duration_ms,
            prefill_tokens = EXCLUDED.prefill_tokens,
            prefill_duration_ms = EXCLUDED.prefill_duration_ms,
            decode_tokens = EXCLUDED.decode_tokens,
            decode_duration_ms = EXCLUDED.decode_duration_ms,
            first_token_ms = EXCLUDED.first_token_ms,
            total_duration_ms = EXCLUDED.total_duration_ms,
            tokens_per_second = EXCLUDED.tokens_per_second,
            rss_before_bytes = EXCLUDED.rss_before_bytes,
            rss_after_load_bytes = EXCLUDED.rss_after_load_bytes,
            rss_peak_bytes = EXCLUDED.rss_peak_bytes,
            failure_message = EXCLUDED.failure_message,
            notes_json = EXCLUDED.notes_json,
            updated_ts = EXCLUDED.updated_ts
        RETURNING id, host_fingerprint, model_name, model_checksum, model_path, benchmark_label,
                  backend_kind, n_threads, n_gpu_layers, split_mode, main_gpu, device_indices_json,
                  load_duration_ms, prefill_tokens, prefill_duration_ms, decode_tokens,
                  decode_duration_ms, first_token_ms, total_duration_ms, tokens_per_second,
                  rss_before_bytes, rss_after_load_bytes, rss_peak_bytes, failure_message, notes_json,
                  created_ts, updated_ts",
    )
    .bind(&id)
    .bind(params.host_fingerprint)
    .bind(params.model_name)
    .bind(params.model_checksum)
    .bind(params.model_path)
    .bind(params.benchmark_label)
    .bind(params.backend_kind)
    .bind(params.n_threads)
    .bind(params.n_gpu_layers)
    .bind(params.split_mode)
    .bind(params.main_gpu)
    .bind(params.device_indices_json)
    .bind(params.load_duration_ms)
    .bind(params.prefill_tokens)
    .bind(params.prefill_duration_ms)
    .bind(params.decode_tokens)
    .bind(params.decode_duration_ms)
    .bind(params.first_token_ms)
    .bind(params.total_duration_ms)
    .bind(params.tokens_per_second)
    .bind(params.rss_before_bytes)
    .bind(params.rss_after_load_bytes)
    .bind(params.rss_peak_bytes)
    .bind(params.failure_message)
    .bind(params.notes_json)
    .bind(now)
    .bind(now)
    .fetch_one(&mut **tx)
    .await?;

    map_benchmark_row(&row)
}

async fn upsert_model_profile_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    params: UpsertAiModelProfileParams<'_>,
) -> Result<AiModelProfileRow, sqlx::Error> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let row = sqlx::query(
        "INSERT INTO ai_model_profile (
            id, host_fingerprint, model_name, model_checksum, model_path, context_window,
            preferred_completion_tokens, planner_max_output, summary_max_output, safety_headroom,
            warmup_cost_class, supports_structured_output, supports_prompt_cache,
            recommended_n_threads, recommended_n_gpu_layers, recommended_split_mode,
            recommended_main_gpu, recommended_device_indices_json, estimated_model_bytes, notes_json,
            last_benchmark_label, last_load_duration_ms, last_tokens_per_second, benchmark_count,
            created_ts, updated_ts
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,$25,$26)
        ON CONFLICT (host_fingerprint, model_checksum)
        DO UPDATE SET
            model_name = EXCLUDED.model_name,
            model_path = EXCLUDED.model_path,
            context_window = EXCLUDED.context_window,
            preferred_completion_tokens = EXCLUDED.preferred_completion_tokens,
            planner_max_output = EXCLUDED.planner_max_output,
            summary_max_output = EXCLUDED.summary_max_output,
            safety_headroom = EXCLUDED.safety_headroom,
            warmup_cost_class = EXCLUDED.warmup_cost_class,
            supports_structured_output = EXCLUDED.supports_structured_output,
            supports_prompt_cache = EXCLUDED.supports_prompt_cache,
            recommended_n_threads = EXCLUDED.recommended_n_threads,
            recommended_n_gpu_layers = EXCLUDED.recommended_n_gpu_layers,
            recommended_split_mode = EXCLUDED.recommended_split_mode,
            recommended_main_gpu = EXCLUDED.recommended_main_gpu,
            recommended_device_indices_json = EXCLUDED.recommended_device_indices_json,
            estimated_model_bytes = EXCLUDED.estimated_model_bytes,
            notes_json = EXCLUDED.notes_json,
            last_benchmark_label = EXCLUDED.last_benchmark_label,
            last_load_duration_ms = EXCLUDED.last_load_duration_ms,
            last_tokens_per_second = EXCLUDED.last_tokens_per_second,
            benchmark_count = EXCLUDED.benchmark_count,
            updated_ts = EXCLUDED.updated_ts
        RETURNING id, host_fingerprint, model_name, model_checksum, model_path, context_window,
                  preferred_completion_tokens, planner_max_output, summary_max_output, safety_headroom,
                  warmup_cost_class, supports_structured_output, supports_prompt_cache,
                  recommended_n_threads, recommended_n_gpu_layers, recommended_split_mode,
                  recommended_main_gpu, recommended_device_indices_json, estimated_model_bytes,
                  notes_json, last_benchmark_label, last_load_duration_ms, last_tokens_per_second,
                  benchmark_count, created_ts, updated_ts",
    )
    .bind(&id)
    .bind(params.host_fingerprint)
    .bind(params.model_name)
    .bind(params.model_checksum)
    .bind(params.model_path)
    .bind(params.context_window)
    .bind(params.preferred_completion_tokens)
    .bind(params.planner_max_output)
    .bind(params.summary_max_output)
    .bind(params.safety_headroom)
    .bind(params.warmup_cost_class)
    .bind(params.supports_structured_output)
    .bind(params.supports_prompt_cache)
    .bind(params.recommended_n_threads)
    .bind(params.recommended_n_gpu_layers)
    .bind(params.recommended_split_mode)
    .bind(params.recommended_main_gpu)
    .bind(params.recommended_device_indices_json)
    .bind(params.estimated_model_bytes)
    .bind(params.notes_json)
    .bind(params.last_benchmark_label)
    .bind(params.last_load_duration_ms)
    .bind(params.last_tokens_per_second)
    .bind(params.benchmark_count)
    .bind(now)
    .bind(now)
    .fetch_one(&mut **tx)
    .await?;

    map_profile_row(&row)
}

pub async fn upsert_model_benchmark(
    pool: &DbPool,
    params: UpsertAiModelBenchmarkParams<'_>,
) -> Result<AiModelBenchmarkRow, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let row = upsert_model_benchmark_in_tx(&mut tx, params).await?;
    tx.commit().await?;
    Ok(row)
}

pub async fn list_model_benchmarks_for_host(
    pool: &DbPool,
    host_fingerprint: &str,
    limit: i64,
) -> Result<Vec<AiModelBenchmarkRow>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, host_fingerprint, model_name, model_checksum, model_path, benchmark_label,
                backend_kind, n_threads, n_gpu_layers, split_mode, main_gpu, device_indices_json,
                load_duration_ms, prefill_tokens, prefill_duration_ms, decode_tokens, decode_duration_ms,
                first_token_ms, total_duration_ms, tokens_per_second, rss_before_bytes, rss_after_load_bytes,
                rss_peak_bytes, failure_message, notes_json, created_ts, updated_ts
         FROM ai_model_benchmark
         WHERE host_fingerprint = $1
         ORDER BY updated_ts DESC, model_name ASC, benchmark_label ASC
         LIMIT $2",
    )
    .bind(host_fingerprint)
    .bind(limit.clamp(1, 500))
    .fetch_all(pool)
    .await?;

    rows.iter().map(map_benchmark_row).collect()
}

pub async fn upsert_model_profile(
    pool: &DbPool,
    params: UpsertAiModelProfileParams<'_>,
) -> Result<AiModelProfileRow, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let row = upsert_model_profile_in_tx(&mut tx, params).await?;
    tx.commit().await?;
    Ok(row)
}

pub async fn upsert_benchmark_and_profile(
    pool: &DbPool,
    benchmark_params: UpsertAiModelBenchmarkParams<'_>,
    profile_params: Option<UpsertAiModelProfileParams<'_>>,
) -> Result<(AiModelBenchmarkRow, Option<AiModelProfileRow>), sqlx::Error> {
    let mut tx = pool.begin().await?;
    let benchmark = upsert_model_benchmark_in_tx(&mut tx, benchmark_params).await?;
    let profile = if let Some(params) = profile_params {
        Some(upsert_model_profile_in_tx(&mut tx, params).await?)
    } else {
        None
    };
    tx.commit().await?;
    Ok((benchmark, profile))
}

pub async fn list_model_profiles_for_host(
    pool: &DbPool,
    host_fingerprint: &str,
    limit: i64,
) -> Result<Vec<AiModelProfileRow>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, host_fingerprint, model_name, model_checksum, model_path, context_window,
                preferred_completion_tokens, planner_max_output, summary_max_output, safety_headroom,
                warmup_cost_class, supports_structured_output, supports_prompt_cache,
                recommended_n_threads, recommended_n_gpu_layers, recommended_split_mode,
                recommended_main_gpu, recommended_device_indices_json, estimated_model_bytes, notes_json,
                last_benchmark_label, last_load_duration_ms, last_tokens_per_second, benchmark_count,
                created_ts, updated_ts
         FROM ai_model_profile
         WHERE host_fingerprint = $1
         ORDER BY updated_ts DESC, model_name ASC
         LIMIT $2",
    )
    .bind(host_fingerprint)
    .bind(limit.clamp(1, 200))
    .fetch_all(pool)
    .await?;

    rows.iter().map(map_profile_row).collect()
}
