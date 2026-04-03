import { apiFetch, apiJson } from './api';
import type { AiGroundingChunk, AiModel } from './aiApi';

export interface AiRemoteBackendState {
  enabled: boolean;
  base_url: string | null;
  model: string | null;
  api_key_env: string | null;
  timeout_secs: number;
  supports_prompt_cache: boolean;
  supports_structured_output: boolean;
  max_parallel_requests: number;
  overload_fallback: boolean;
  route_roles: string[];
}

export interface AiSchedulerPriorityCount {
  priority: string;
  count: number;
}

export interface AiSchedulerWarmModel {
  model_name: string;
  estimated_bytes: number;
  loaded_ts_ms: number;
  last_used_ts_ms: number;
  load_count: number;
}

export interface AiSchedulerState {
  max_concurrent_turns: number;
  queue_limit: number;
  active_turns: number;
  queued_turns: number;
  overload_state: string;
  warm_pool_bytes: number;
  warm_pool_budget_bytes: number;
  active_by_priority: AiSchedulerPriorityCount[];
  queued_by_priority: AiSchedulerPriorityCount[];
  warm_models: AiSchedulerWarmModel[];
  rejected_turns_total: number;
  degraded_turns_total: number;
}

export interface AiModelBenchmarkSummary {
  id: string;
  model_name: string;
  model_checksum: string;
  benchmark_label: string;
  backend_kind: string;
  n_threads: number;
  n_gpu_layers: number;
  split_mode: string;
  main_gpu?: number | null;
  load_duration_ms: number;
  prefill_tokens: number;
  prefill_duration_ms: number;
  decode_tokens: number;
  decode_duration_ms: number;
  first_token_ms: number;
  total_duration_ms: number;
  tokens_per_second: number;
  failure_message?: string | null;
  created_ts: number;
  updated_ts: number;
}

export interface AiModelProfileSummary {
  id: string;
  model_name: string;
  model_checksum: string;
  context_window: number;
  preferred_completion_tokens: number;
  planner_max_output: number;
  summary_max_output: number;
  safety_headroom: number;
  warmup_cost_class: string;
  supports_structured_output: boolean;
  supports_prompt_cache: boolean;
  recommended_n_threads: number;
  recommended_n_gpu_layers: number;
  recommended_split_mode: string;
  recommended_main_gpu?: number | null;
  estimated_model_bytes: number;
  last_benchmark_label: string;
  last_load_duration_ms: number;
  last_tokens_per_second: number;
  benchmark_count: number;
  created_ts: number;
  updated_ts: number;
}

export interface AiRoleRoutingDecision {
  role: 'planner' | 'summarizer' | 'answer' | 'verifier' | 'worker' | string;
  model_name: string;
  backend_id: string;
  backend_kind: 'local' | 'remote' | string;
  selection_source:
    | 'explicit_request'
    | 'stored_recommendation'
    | 'env_default'
    | 'fallback'
    | string;
  recommendation_status:
    | 'applied'
    | 'missing'
    | 'stale'
    | 'model_missing'
    | 'not_applicable'
    | string;
  recommendation_note?: string | null;
  recommendation_model_name?: string | null;
  recommendation_updated_ts?: number | null;
}

export interface AiAdminState {
  available: boolean;
  model_dir: string;
  default_model_dir: string;
  model_dir_source: 'database' | 'environment' | 'default' | 'default_fallback';
  model_storage_available: boolean;
  model_storage_error: string | null;
  audit_retention_days: number;
  audit_prune_interval_seconds: number;
  models: AiModel[];
  remote_backend?: AiRemoteBackendState | null;
  scheduler?: AiSchedulerState;
  model_benchmarks?: AiModelBenchmarkSummary[];
  model_profiles?: AiModelProfileSummary[];
  role_routing?: AiRoleRoutingDecision[];
}

export interface AiAssistantAuditGroundingSource {
  tool: string;
  label: string;
  access_mode: string;
  risk_tier: string;
  status: string;
}

export interface AiAssistantAuditToolExecution {
  tool: string;
  input_summary: string;
  status: string;
  label: string;
  result_count: number | null;
}

export interface AiAssistantPlannerAuditExecution {
  parse_attempts?: number;
  validation_failures?: number;
  repair_attempts?: number;
  repair_successes?: number;
  fallback_reason?: string | null;
}

export interface AiAssistantPlannerAudit {
  raw_response_hash?: string | null;
  planner_mode?: string | null;
  fallback_reason?: string | null;
  repair_attempt_count?: number;
  final_selected_tools?: string[];
  validation_errors?: string[];
  execution?: AiAssistantPlannerAuditExecution;
}

export interface AiAssistantAuditEvent {
  id: string;
  trace_id: string;
  user_id: string;
  username: string;
  user_role: string;
  model_name: string;
  message_preview: string;
  history_len: number;
  response_kind: string;
  planner: AiAssistantPlannerAudit;
  model_routing: AiRoleRoutingDecision[];
  planned_tools: string[];
  executed_tools: AiAssistantAuditToolExecution[];
  grounding_chunks: AiGroundingChunk[];
  grounding_sources: AiAssistantAuditGroundingSource[];
  error_message: string | null;
  created_ts: number;
}

export type AdminAiPullEvent =
  | { type: 'progress'; status: string; bytes_done: number; bytes_total: number | null; percent: number }
  | { type: 'done' }
  | { type: 'error'; message: string };

export async function fetchAiAdminState(): Promise<AiAdminState> {
  return apiJson<AiAdminState>('/system/ai');
}

export async function updateAiModelDir(modelDir: string): Promise<AiAdminState> {
  return apiJson<AiAdminState>('/system/ai', {
    method: 'PUT',
    body: JSON.stringify({ model_dir: modelDir }),
  });
}

export async function updateAiRemoteBackend(config: {
  enabled: boolean;
  base_url: string;
  model: string;
  api_key_env?: string | null;
  timeout_secs?: number;
  supports_prompt_cache?: boolean;
  supports_structured_output?: boolean;
  max_parallel_requests?: number;
  overload_fallback?: boolean;
  route_roles?: string[];
}): Promise<AiAdminState> {
  return apiJson<AiAdminState>('/system/ai/backend', {
    method: 'PUT',
    body: JSON.stringify(config),
  });
}

export async function runAiModelBenchmark(config: {
  model_name?: string | null;
  benchmark_label?: string | null;
}): Promise<AiAdminState> {
  return apiJson<AiAdminState>('/system/ai/benchmarks/run', {
    method: 'POST',
    body: JSON.stringify(config),
  });
}

export async function fetchAiAuditEvents(limit = 40): Promise<AiAssistantAuditEvent[]> {
  const params = new URLSearchParams();
  params.set('limit', String(limit));
  return apiJson<AiAssistantAuditEvent[]>(`/system/ai/audit?${params.toString()}`);
}

export async function deleteAiModel(name: string): Promise<void> {
  const encoded = encodeURIComponent(name);
  const res = await apiFetch(`/system/ai/models/${encoded}`, { method: 'DELETE' });
  if (!res.ok && res.status !== 204 && res.status !== 404) {
    throw new Error(`Delete failed: ${res.status}`);
  }
}

export function pullAiModelFromUrl(
  url: string,
  onEvent: (event: AdminAiPullEvent) => void,
  onClose: () => void,
): () => void {
  const controller = new AbortController();

  (async () => {
    let res: Response;
    try {
      res = await apiFetch('/system/ai/models/pull', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ url }),
        signal: controller.signal,
      });
    } catch {
      onEvent({ type: 'error', message: 'Failed to connect.' });
      onClose();
      return;
    }

    if (!res.ok || !res.body) {
      onEvent({ type: 'error', message: `Server returned ${res.status}` });
      onClose();
      return;
    }

    const reader = res.body.getReader();
    const decoder = new TextDecoder();
    let buf = '';

    try {
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        buf += decoder.decode(value, { stream: true });
        const lines = buf.split('\n');
        buf = lines.pop() ?? '';

        let eventType = '';
        for (const line of lines) {
          if (line.startsWith('event: ')) {
            eventType = line.slice(7).trim();
          } else if (line.startsWith('data: ')) {
            const raw = line.slice(6).trim();
            try {
              const parsed = JSON.parse(raw);
              if (eventType === 'progress') {
                onEvent({
                  type: 'progress',
                  status: parsed.status ?? '',
                  bytes_done: parsed.bytes_done ?? 0,
                  bytes_total: parsed.bytes_total ?? null,
                  percent: parsed.percent ?? 0,
                });
              } else if (eventType === 'done') {
                onEvent({ type: 'done' });
              } else if (eventType === 'error') {
                onEvent({ type: 'error', message: parsed.message ?? 'Unknown error' });
              }
            } catch {
              // Ignore malformed SSE payloads.
            }
            eventType = '';
          }
        }
      }
    } catch {
      // Ignore abort/broken stream.
    } finally {
      onClose();
    }
  })();

  return () => controller.abort();
}
