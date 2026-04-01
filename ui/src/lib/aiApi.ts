import { apiFetch, apiJson, extractErrorMessage, parseResponseBody } from './api';

// ---------- Types ------------------------------------------------------------

export interface AiModel {
  name: string;
  file: string;
  size_gb: number;
  parameter_size: string | null;
  quantization: string | null;
  architecture: string | null;
  context_length: number | null;
}

export interface ModelsResponse {
  models: AiModel[];
  inference_available: boolean;
  model_storage_available: boolean;
  model_storage_error: string | null;
  service_unavailable: boolean;
}

export interface ChatHistoryMessage {
  role: 'user' | 'assistant';
  content: string;
  grounding_tools?: string[];
  follow_up_contexts?: AiFollowUpContext[];
}

export interface AiGroundingSource {
  tool: string;
  label: string;
  access_mode: 'read_only' | 'write' | 'destructive_write';
  risk_tier: 'low' | 'moderate' | 'high' | 'critical';
  status: string;
}

export interface AiFollowUpEntity {
  ordinal: number;
  label: string;
  identifier?: string | null;
}

export interface AiFollowUpInputHint {
  calendar_label?: string | null;
  calendar_from_date?: string | null;
  calendar_to_date?: string | null;
  calendar_query?: string | null;
  channels_query?: string | null;
  downloads_query?: string | null;
  downloads_availability?: string | null;
  room_mode?: string | null;
  room_query?: string | null;
  server_query?: string | null;
  server_availability?: string | null;
  library_query?: string | null;
  weather_location?: string | null;
  weather_days?: number | null;
  web_search_query?: string | null;
  web_url?: string | null;
}

export interface AiFollowUpContext {
  tool: string;
  label: string;
  input_hint?: AiFollowUpInputHint;
  entities: AiFollowUpEntity[];
}

export interface AiTurnStats {
  prompt_tokens: number;
  completion_tokens: number;
  total_duration_ms: number;
  generation_duration_ms: number;
  planner_duration_ms: number;
  tool_duration_ms: number;
  end_to_end_duration_ms: number;
  queue_duration_ms: number;
  model_load_duration_ms: number;
  tokens_per_second: number;
}

export type AiPhase = 'planning' | 'generating';

export interface AiPhaseEvent {
  phase: AiPhase;
  label: string;
  started_ts_ms: number;
  finished_ts_ms?: number | null;
}

export type AiToolActivityState = 'running' | 'complete' | 'error';

export interface AiToolActivityEvent {
  id: string;
  tool: string;
  label: string;
  state: AiToolActivityState;
  started_ts_ms: number;
  finished_ts_ms?: number | null;
}

export type AiActivityTraceItem =
  | ({
      kind: 'phase';
    } & AiPhaseEvent)
  | ({
      kind: 'tool';
    } & AiToolActivityEvent);

export interface AiConversationSummary {
  id: string;
  title: string;
  last_message_preview?: string | null;
  last_model_name?: string | null;
  updated_ts: number;
  archived: boolean;
}

export interface AiConversationTurn {
  id: string;
  role: 'user' | 'assistant';
  content: string;
  model_name?: string | null;
  grounding_tools: string[];
  follow_up_contexts: AiFollowUpContext[];
  grounding_sources: AiGroundingSource[];
  activity_trace: AiActivityTraceItem[];
  stats?: AiTurnStats | null;
  created_ts: number;
}

export interface AiConversationDetail {
  id: string;
  title: string;
  archived: boolean;
  last_message_preview?: string | null;
  last_model_name?: string | null;
  created_ts: number;
  updated_ts: number;
  messages: AiConversationTurn[];
}

export interface AiStatusUpdate {
  tool: string;
  label: string;
  kind: 'checking' | 'complete' | 'error';
}

export type AiSseEvent =
  | { type: 'phase'; phase: AiPhaseEvent }
  | { type: 'tool'; activity: AiToolActivityEvent }
  | { type: 'status'; update: AiStatusUpdate }
  | { type: 'token'; text: string }
  | { type: 'stats'; stats: AiTurnStats }
  | { type: 'grounding'; sources: AiGroundingSource[]; followUpContexts: AiFollowUpContext[] }
  | { type: 'done' }
  | { type: 'error'; message: string };

interface ConversationListResponse {
  conversations?: AiConversationSummary[];
}

interface ConversationResponse {
  conversation?: AiConversationDetail;
}

// ---------- Helpers ----------------------------------------------------------

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function readString(value: unknown, fallback = ''): string {
  return typeof value === 'string' ? value : fallback;
}

function readNumber(value: unknown, fallback = 0): number {
  return typeof value === 'number' && Number.isFinite(value) ? value : fallback;
}

function normalizeTurnStats(value: unknown): AiTurnStats | null {
  if (!isRecord(value)) return null;
  return {
    prompt_tokens: readNumber(value.prompt_tokens),
    completion_tokens: readNumber(value.completion_tokens),
    total_duration_ms: readNumber(value.total_duration_ms),
    generation_duration_ms: readNumber(value.generation_duration_ms),
    planner_duration_ms: readNumber(value.planner_duration_ms),
    tool_duration_ms: readNumber(value.tool_duration_ms),
    end_to_end_duration_ms: readNumber(value.end_to_end_duration_ms),
    queue_duration_ms: readNumber(value.queue_duration_ms),
    model_load_duration_ms: readNumber(value.model_load_duration_ms),
    tokens_per_second: readNumber(value.tokens_per_second),
  };
}

function handleSsePayload(
  eventType: string,
  payload: unknown,
  onEvent: (event: AiSseEvent) => void,
) {
  if (eventType === 'token') {
    const record = isRecord(payload) ? payload : {};
    onEvent({ type: 'token', text: readString(record.text) });
    return;
  }

  if (eventType === 'status') {
    const record = isRecord(payload) ? payload : {};
    onEvent({
      type: 'status',
      update: {
        tool: readString(record.tool),
        label: readString(record.label),
        kind:
          record.kind === 'complete' || record.kind === 'error'
            ? record.kind
            : 'checking',
      },
    });
    return;
  }

  if (eventType === 'phase') {
    const record = isRecord(payload) ? payload : {};
    const phase = record.phase === 'generating' ? 'generating' : 'planning';
    onEvent({
      type: 'phase',
      phase: {
        phase,
        label: readString(record.label, 'Thinking...'),
        started_ts_ms: readNumber(record.started_ts_ms, Date.now()),
        finished_ts_ms:
          typeof record.finished_ts_ms === 'number' ? record.finished_ts_ms : null,
      },
    });
    return;
  }

  if (eventType === 'tool') {
    const record = isRecord(payload) ? payload : {};
    const state =
      record.state === 'complete' || record.state === 'error'
        ? record.state
        : 'running';
    onEvent({
      type: 'tool',
      activity: {
        id: readString(record.id),
        tool: readString(record.tool),
        label: readString(record.label),
        state,
        started_ts_ms: readNumber(record.started_ts_ms, Date.now()),
        finished_ts_ms:
          typeof record.finished_ts_ms === 'number' ? record.finished_ts_ms : null,
      },
    });
    return;
  }

  if (eventType === 'stats') {
    const stats = normalizeTurnStats(payload);
    if (stats) {
      onEvent({ type: 'stats', stats });
    }
    return;
  }

  if (eventType === 'grounding') {
    const record = isRecord(payload) ? payload : {};
    onEvent({
      type: 'grounding',
      sources: Array.isArray(record.sources)
        ? (record.sources as AiGroundingSource[])
        : [],
      followUpContexts: Array.isArray(record.follow_up_contexts)
        ? (record.follow_up_contexts as AiFollowUpContext[])
        : [],
    });
    return;
  }

  if (eventType === 'done') {
    onEvent({ type: 'done' });
    return;
  }

  if (eventType === 'error') {
    const record = isRecord(payload) ? payload : {};
    onEvent({
      type: 'error',
      message: readString(record.message, 'Unknown error'),
    });
  }
}

function streamAssistantRequest(
  path: string,
  body: unknown,
  onEvent: (event: AiSseEvent) => void,
  onClose: () => void,
): () => void {
  const controller = new AbortController();

  void (async () => {
    let res: Response;
    try {
      res = await apiFetch(path, {
        method: 'POST',
        body: JSON.stringify(body),
        signal: controller.signal,
      });
    } catch {
      if (!controller.signal.aborted) {
        onEvent({ type: 'error', message: 'Failed to connect to AI service.' });
      }
      onClose();
      return;
    }

    if (!res.ok || !res.body) {
      const errorBody = await parseResponseBody(res).catch(() => undefined);
      onEvent({
        type: 'error',
        message: extractErrorMessage(errorBody, `Server returned ${res.status}`),
      });
      onClose();
      return;
    }

    const reader = res.body.getReader();
    const decoder = new TextDecoder();
    let buffer = '';

    const processBlock = (block: string) => {
      if (!block.trim()) return;
      let eventType = '';
      const dataLines: string[] = [];
      for (const rawLine of block.split('\n')) {
        const line = rawLine.trimEnd();
        if (!line || line.startsWith(':')) continue;
        if (line.startsWith('event:')) {
          eventType = line.slice(6).trim();
          continue;
        }
        if (line.startsWith('data:')) {
          dataLines.push(line.slice(5).trimStart());
        }
      }

      if (!eventType) return;
      const rawPayload = dataLines.join('\n');
      let payload: unknown = null;
      if (rawPayload) {
        try {
          payload = JSON.parse(rawPayload);
        } catch {
          payload = null;
        }
      }
      handleSsePayload(eventType, payload, onEvent);
    };

    try {
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;

        buffer += decoder.decode(value, { stream: true }).replace(/\r\n/g, '\n');
        let boundary = buffer.indexOf('\n\n');
        while (boundary >= 0) {
          processBlock(buffer.slice(0, boundary));
          buffer = buffer.slice(boundary + 2);
          boundary = buffer.indexOf('\n\n');
        }
      }

      buffer += decoder.decode().replace(/\r\n/g, '\n');
      if (buffer.trim()) {
        processBlock(buffer);
      }
    } catch {
      if (!controller.signal.aborted) {
        onEvent({ type: 'error', message: 'AI stream ended unexpectedly.' });
      }
    } finally {
      onClose();
    }
  })();

  return () => controller.abort();
}

// ---------- Model management -------------------------------------------------

export async function fetchModels(): Promise<ModelsResponse> {
  const res = await apiFetch('/ai/models');
  const body = (await parseResponseBody(res).catch(() => null)) as
    | {
        models?: AiModel[];
        inference_available?: boolean;
        model_storage_available?: boolean;
        model_storage_error?: string | null;
        error?: { message?: string };
      }
    | null;

  if (res.status === 503) {
    return {
      models: [],
      inference_available: false,
      model_storage_available: false,
      model_storage_error: body?.error?.message ?? 'AI is unavailable on this host.',
      service_unavailable: true,
    };
  }

  if (!res.ok) {
    return {
      models: [],
      inference_available: false,
      model_storage_available: false,
      model_storage_error:
        body?.error?.message ?? `Failed to fetch AI models: ${res.status}`,
      service_unavailable: false,
    };
  }

  return {
    models: body?.models ?? [],
    inference_available: Boolean(body?.inference_available),
    model_storage_available: body?.model_storage_available !== false,
    model_storage_error: body?.model_storage_error ?? null,
    service_unavailable: false,
  };
}

// ---------- Conversations ----------------------------------------------------

export async function listConversations(
  includeArchived = true,
): Promise<AiConversationSummary[]> {
  const params = new URLSearchParams();
  if (includeArchived) {
    params.set('include_archived', 'true');
  }
  const suffix = params.size > 0 ? `?${params.toString()}` : '';
  const body = await apiJson<ConversationListResponse>(`/ai/conversations${suffix}`);
  return body.conversations ?? [];
}

export async function createConversation(
  title?: string,
): Promise<AiConversationDetail> {
  const body = await apiJson<ConversationResponse>('/ai/conversations', {
    method: 'POST',
    body: JSON.stringify(title ? { title } : {}),
  });
  if (!body.conversation) {
    throw new Error('Conversation response was empty.');
  }
  return body.conversation;
}

export async function getConversation(
  conversationId: string,
): Promise<AiConversationDetail> {
  const body = await apiJson<ConversationResponse>(
    `/ai/conversations/${conversationId}`,
  );
  if (!body.conversation) {
    throw new Error('Conversation response was empty.');
  }
  return body.conversation;
}

export async function updateConversation(
  conversationId: string,
  updates: { title?: string; archived?: boolean },
): Promise<AiConversationDetail> {
  const body = await apiJson<ConversationResponse>(
    `/ai/conversations/${conversationId}`,
    {
      method: 'PATCH',
      body: JSON.stringify(updates),
    },
  );
  if (!body.conversation) {
    throw new Error('Conversation response was empty.');
  }
  return body.conversation;
}

export async function deleteConversation(conversationId: string): Promise<void> {
  await apiJson<void>(`/ai/conversations/${conversationId}`, {
    method: 'DELETE',
  });
}

// ---------- Chat streaming ---------------------------------------------------

export function streamChat(
  model: string,
  message: string,
  history: ChatHistoryMessage[],
  onEvent: (event: AiSseEvent) => void,
  onClose: () => void,
): () => void {
  return streamAssistantRequest(
    '/ai/chat',
    { model, message, history },
    onEvent,
    onClose,
  );
}

export function streamConversationMessage(
  conversationId: string,
  model: string,
  message: string,
  onEvent: (event: AiSseEvent) => void,
  onClose: () => void,
): () => void {
  return streamAssistantRequest(
    `/ai/conversations/${conversationId}/messages/stream`,
    { model, message },
    onEvent,
    onClose,
  );
}
