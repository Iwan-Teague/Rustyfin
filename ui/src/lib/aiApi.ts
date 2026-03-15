import { readBrowserToken } from './browserAuth';

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
}

export interface ChatHistoryMessage {
  role: 'user' | 'assistant';
  content: string;
}

export type AiSseEvent =
  | { type: 'token'; text: string }
  | { type: 'stats'; prompt_tokens: number; completion_tokens: number; total_duration_ms: number; tokens_per_second: number }
  | { type: 'done' }
  | { type: 'error'; message: string };

// ---------- Helpers ----------------------------------------------------------

function authHeaders(): Record<string, string> {
  const token = readBrowserToken();
  return token ? { Authorization: `Bearer ${token}` } : {};
}

// ---------- Model management -------------------------------------------------

export async function fetchModels(): Promise<ModelsResponse> {
  const res = await fetch('/api/v1/ai/models', { headers: authHeaders() });
  if (res.status === 503) {
    return { models: [], inference_available: false };
  }
  if (!res.ok) throw new Error(`Failed to fetch models: ${res.status}`);
  const body = await res.json();
  return {
    models: body.models ?? [],
    inference_available: Boolean(body.inference_available),
  };
}

// ---------- Chat streaming ---------------------------------------------------

export function streamChat(
  model: string,
  message: string,
  history: ChatHistoryMessage[],
  onEvent: (event: AiSseEvent) => void,
  onClose: () => void,
): () => void {
  const controller = new AbortController();

  (async () => {
    let res: Response;
    try {
      res = await fetch('/api/v1/ai/chat', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', ...authHeaders() },
        body: JSON.stringify({ model, message, history }),
        signal: controller.signal,
      });
    } catch {
      onEvent({ type: 'error', message: 'Failed to connect to AI service.' });
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
              const payload = JSON.parse(raw);
              if (eventType === 'token') {
                onEvent({ type: 'token', text: payload.text ?? '' });
              } else if (eventType === 'stats') {
                onEvent({
                  type: 'stats',
                  prompt_tokens: payload.prompt_tokens ?? 0,
                  completion_tokens: payload.completion_tokens ?? 0,
                  total_duration_ms: payload.total_duration_ms ?? 0,
                  tokens_per_second: payload.tokens_per_second ?? 0,
                });
              } else if (eventType === 'done') {
                onEvent({ type: 'done' });
              } else if (eventType === 'error') {
                onEvent({ type: 'error', message: payload.message ?? 'Unknown error' });
              }
            } catch {
              // skip malformed
            }
            eventType = '';
          }
        }
      }
    } catch {
      // aborted or broken
    } finally {
      onClose();
    }
  })();

  return () => controller.abort();
}
