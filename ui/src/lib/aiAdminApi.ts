import { apiFetch, apiJson } from './api';
import type { AiGroundingChunk, AiModel } from './aiApi';

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
  planned_tools: string[];
  executed_tools: AiAssistantAuditToolExecution[];
  grounding_chunks: AiGroundingChunk[];
  grounding_sources: AiAssistantAuditGroundingSource[];
  error_message: string | null;
  created_ts: number;
}

export interface AiTurnJournal {
  id: string;
  user_id: string;
  conversation_id?: string | null;
  request_turn_id?: string | null;
  request_turn_index?: number | null;
  trace_id: string;
  request_message: string;
  model_name: string;
  response_mode: string;
  planner_mode?: string | null;
  status: string;
  current_phase: string;
  history_len: number;
  planner_debug: {
    schema_version: number;
    raw_response?: string | null;
    repaired_response?: string | null;
    validation_errors: string[];
    repair_attempt_count: number;
    used_repaired_response: boolean;
    validated_call_count: number;
  };
  prompt_debug?: {
    context_length: number;
    prompt_budget_tokens: number;
    reserved_completion_tokens: number;
    prompt_tokens_estimate: number;
    loaded_history_turns: number;
    retained_raw_turns: number;
    summarized_turns: number;
    recent_grounded_context_count: number;
    used_memory_summary: boolean;
    memory_turn_index: number;
    memory_summary_chars: number;
    compact_boundary_count: number;
    recovered_from_compact_boundary: boolean;
  } | null;
  stats?: {
    planner_repair_count: number;
    planner_validation_error_count: number;
    prompt_budget_tokens: number;
    reserved_completion_tokens: number;
    completion_budget_tokens: number;
    compact_boundary_count: number;
    overload: boolean;
    overload_reason?: string | null;
    artifact_verification_attempts: number;
    artifact_revision_count: number;
  } | null;
  overload_reason?: string | null;
  error_message?: string | null;
  compact_boundary_count: number;
  artifact_verification?: {
    status: string;
    attempts: number;
    revision_count: number;
    issues: string[];
  } | null;
  created_ts: number;
  updated_ts: number;
  finished_ts?: number | null;
}

export interface AiCompactBoundary {
  id: string;
  conversation_id: string;
  user_id: string;
  trace_id?: string | null;
  from_turn_index: number;
  to_turn_index: number;
  summarized_turn_count: number;
  memory_state_json: string;
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

export async function fetchAiAuditEvents(limit = 40): Promise<AiAssistantAuditEvent[]> {
  const params = new URLSearchParams();
  params.set('limit', String(limit));
  return apiJson<AiAssistantAuditEvent[]>(`/system/ai/audit?${params.toString()}`);
}

export async function fetchAiTurnJournals(limit = 30): Promise<AiTurnJournal[]> {
  const params = new URLSearchParams();
  params.set('limit', String(limit));
  return apiJson<AiTurnJournal[]>(`/system/ai/journals?${params.toString()}`);
}

export async function fetchAiCompactBoundaries(limit = 20): Promise<AiCompactBoundary[]> {
  const params = new URLSearchParams();
  params.set('limit', String(limit));
  return apiJson<AiCompactBoundary[]>(`/system/ai/compact-boundaries?${params.toString()}`);
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
