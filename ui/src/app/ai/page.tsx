'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { useRouter } from 'next/navigation';

import ConfirmModal from '@/app/components/ConfirmModal';
import AiAssistantActivity from '@/features/ai-assistant/components/AiAssistantActivity';
import AiConversationRail from '@/features/ai-assistant/components/AiConversationRail';
import { useAuth } from '@/lib/auth';
import {
  type AiActivityTraceItem,
  type AiConversationDetail,
  type AiConversationSummary,
  type AiConversationTurn,
  type AiModel,
  type AiPendingAction,
  type AiPhaseEvent,
  type AiRuntimeResponse,
  type AiStatusUpdate,
  type AiToolActivityEvent,
  type AiTurnStats,
  createConversation,
  deleteConversation,
  fetchModels,
  fetchAiRuntime,
  getConversation,
  listConversations,
  streamConversationMessage,
  transcribeAiInput,
  updateConversation,
} from '@/lib/aiApi';
import { findDataDeleteTarget, playTelegramDeleteAnimation } from '@/lib/deleteAnimation';
import { clientErrorMessage } from '@/lib/errors';

const DEFAULT_CONVERSATION_TITLE = 'New chat';

type UiConversationTurn = AiConversationTurn & {
  isStreaming: boolean;
  errorMessage: string | null;
};

type UiConversationDetail = Omit<AiConversationDetail, 'messages'> & {
  messages: UiConversationTurn[];
};

type ConversationPromptStats = {
  id: string;
  label: string;
  promptTokens: number;
  completionTokens: number;
  generationDurationMs: number;
  totalDurationMs: number;
  tokensPerSecond: number;
};

type ConversationStatsSummary = {
  promptCount: number;
  totalInputTokens: number;
  totalOutputTokens: number;
  averageTokensPerSecond: number;
  averageMsPerToken: number;
  prompts: ConversationPromptStats[];
};

type QueuedPromptMap = Record<string, string>;

type VoiceState = 'idle' | 'recording' | 'stopping' | 'transcribing' | 'error';

type BrowserSpeechRecognitionResult = {
  isFinal: boolean;
  0: {
    transcript: string;
  };
  length: number;
};

type BrowserSpeechRecognitionEvent = {
  results: ArrayLike<BrowserSpeechRecognitionResult>;
};

type BrowserSpeechRecognitionErrorEvent = {
  error: string;
};

type BrowserSpeechRecognition = {
  lang: string;
  continuous: boolean;
  interimResults: boolean;
  maxAlternatives: number;
  onresult: ((event: BrowserSpeechRecognitionEvent) => void) | null;
  onerror: ((event: BrowserSpeechRecognitionErrorEvent) => void) | null;
  onend: (() => void) | null;
  start: () => void;
  stop: () => void;
  abort: () => void;
};

type BrowserSpeechRecognitionConstructor = new () => BrowserSpeechRecognition;

declare global {
  interface Window {
    SpeechRecognition?: BrowserSpeechRecognitionConstructor;
    webkitSpeechRecognition?: BrowserSpeechRecognitionConstructor;
  }
}

function uid(prefix: string): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return `${prefix}-${crypto.randomUUID()}`;
  }
  return `${prefix}-${Math.random().toString(36).slice(2, 10)}`;
}

function nowTsSeconds(): number {
  return Math.floor(Date.now() / 1000);
}

function normalizePreview(message: string): string {
  const normalized = message
    .split(/\s+/)
    .join(' ')
    .trim();
  if (!normalized) return '(empty message)';
  if (normalized.length <= 280) return normalized;
  return `${normalized.slice(0, 280)}...`;
}

function suggestedConversationTitle(message: string): string {
  const preview = normalizePreview(message);
  if (preview.length <= 80) return preview;
  return `${preview.slice(0, 80)}...`;
}

function sortConversationSummaries(
  conversations: AiConversationSummary[],
): AiConversationSummary[] {
  return [...conversations].sort((left, right) => {
    if (right.updated_ts !== left.updated_ts) {
      return right.updated_ts - left.updated_ts;
    }
    return right.id.localeCompare(left.id);
  });
}

function chooseConversationId(
  conversations: AiConversationSummary[],
  preferredId: string | null,
): string | null {
  if (preferredId && conversations.some((conversation) => conversation.id === preferredId)) {
    return preferredId;
  }
  const preferredVisible = conversations.find((conversation) => !conversation.archived);
  return preferredVisible?.id ?? conversations[0]?.id ?? null;
}

function buildConversationSummary(
  detail: Pick<
    UiConversationDetail,
    'id' | 'title' | 'archived' | 'last_message_preview' | 'last_model_name' | 'updated_ts'
  >,
): AiConversationSummary {
  return {
    id: detail.id,
    title: detail.title,
    last_message_preview: detail.last_message_preview ?? null,
    last_model_name: detail.last_model_name ?? null,
    updated_ts: detail.updated_ts,
    archived: detail.archived,
  };
}

function upsertConversationSummary(
  conversations: AiConversationSummary[],
  summary: AiConversationSummary,
): AiConversationSummary[] {
  const remaining = conversations.filter((conversation) => conversation.id !== summary.id);
  return sortConversationSummaries([summary, ...remaining]);
}

function toUiTurn(turn: AiConversationTurn): UiConversationTurn {
  return {
    ...turn,
    isStreaming: false,
    errorMessage: null,
  };
}

function toUiConversation(detail: AiConversationDetail): UiConversationDetail {
  return {
    ...detail,
    messages: detail.messages.map(toUiTurn),
  };
}

function formatMs(ms: number): string {
  if (ms <= 0) return '0ms';
  return ms < 1000 ? `${ms}ms` : `${(ms / 1000).toFixed(1)}s`;
}

function formatTps(tps: number): string {
  return tps > 0 ? `${tps.toFixed(1)} t/s` : '—';
}

function parseContent(raw: string): { thinking: string | null; content: string } {
  if (!raw.startsWith('<think>')) return { thinking: null, content: raw };
  const closeIdx = raw.indexOf('</think>');
  if (closeIdx === -1) return { thinking: raw.slice(7), content: '' };
  return {
    thinking: raw.slice(7, closeIdx).trim() || null,
    content: raw.slice(closeIdx + 8).trimStart(),
  };
}

function modelDisplayName(name: string): string {
  return name.replace(':', ' · ');
}

const STARTER_SUGGESTIONS = [
  'How much RAM is the server using right now?',
  "What's my next event?",
  'What events are coming up this week?',
  'Who has a birthday coming up?',
  'What was the last call about?',
  'What rooms can I join right now?',
  'Any unread activity in general chat?',
  'What downloads are available right now?',
  'What IP should I use on the local network to open Rustyfin?',
  'What time is it in Italy right now?',
  "What's the weather like in Galway today?",
  'Did it rain yesterday in Galway?',
  'What was recently added to my libraries?',
  'Search my libraries for Star Trek',
  'What public rooms are active right now?',
  'Which invites can I use right now?',
  'What is the next thing coming up in my calendar?',
  'Show me the details for my next calendar event',
  'Which birthdays are coming up this month?',
  'What is the weather this week in Campile, County Wexford?',
  'What temperature is it in Dublin right now?',
  'What rooms can I join with video?',
  'Summarize my most recent voice call',
  'What channels were active recently?',
  'Which library items were added most recently?',
  'What is the current date and time on this Rustyfin host?',
  "When is my next birthday event?",
  "What's the weather tomorrow in Cork?",
  'Show me joinable rooms I can enter right now',
  'Which network address should I use from another device on this LAN?',
  'What open rooms are running right now?',
  'Show my visible calendar events for the next seven days.',
] as const;

const ADMIN_STARTER_SUGGESTIONS = [
  'What services are down right now?',
  'How much storage is free on the NAS?',
  'How much VRAM are the GPUs using right now?',
  'What model is currently loaded?',
  'What is the AI runtime status right now?',
  'What recent backup errors should I know about?',
  'How many active AI requests are running right now?',
  'What recent host errors should I check?',
] as const;

function pickStarterSuggestions(isAdmin: boolean, limit = 10): string[] {
  const pool = [
    ...STARTER_SUGGESTIONS,
    ...(isAdmin ? ADMIN_STARTER_SUGGESTIONS : []),
  ];

  for (let index = pool.length - 1; index > 0; index -= 1) {
    const swapIndex = Math.floor(Math.random() * (index + 1));
    [pool[index], pool[swapIndex]] = [pool[swapIndex], pool[index]];
  }

  return pool.slice(0, Math.min(limit, pool.length));
}

function formatPercent(value: number | null | undefined, digits = 0): string {
  if (typeof value !== 'number' || !Number.isFinite(value)) {
    return '—';
  }
  return `${value.toFixed(digits)}%`;
}

function runtimeSplitModeLabel(splitMode: AiRuntimeResponse['model']['split_mode']): string {
  switch (splitMode) {
    case 'none':
      return 'Single GPU';
    case 'row':
      return 'Tensor split';
    case 'layer':
      return 'Layer split';
    default:
      return splitMode;
  }
}

function runtimeRamSummary(resources: AiRuntimeResponse['resources']): string {
  const used = resources.host_ram_used_human ?? '—';
  const total = resources.host_ram_total_human ?? '—';
  const percent = formatPercent(resources.host_ram_used_percent, 1);
  return `${used} of ${total} (${percent})`;
}

function runtimeDeviceSummary(deviceIndices: number[]): string | null {
  if (deviceIndices.length === 0) {
    return null;
  }
  return `Devices ${deviceIndices.join(', ')}`;
}

function buildConversationStatsSummary(
  conversation: UiConversationDetail | null,
): ConversationStatsSummary | null {
  if (!conversation) return null;

  const prompts: ConversationPromptStats[] = [];
  const userTurns = conversation.messages.filter((message) => message.role === 'user');
  let lastUserContent = 'Prompt';
  let totalInputTokens = 0;
  let totalOutputTokens = 0;
  let totalGenerationDurationMs = 0;

  for (const message of conversation.messages) {
    if (message.role === 'user') {
      lastUserContent = normalizePreview(message.content);
      continue;
    }

    if (!message.stats) {
      continue;
    }

    totalInputTokens += message.stats.prompt_tokens;
    totalOutputTokens += message.stats.completion_tokens;
    totalGenerationDurationMs += message.stats.generation_duration_ms;

    prompts.push({
      id: message.id,
      label: lastUserContent,
      promptTokens: message.stats.prompt_tokens,
      completionTokens: message.stats.completion_tokens,
      generationDurationMs: message.stats.generation_duration_ms,
      totalDurationMs: message.stats.end_to_end_duration_ms || message.stats.total_duration_ms,
      tokensPerSecond: message.stats.tokens_per_second,
    });
  }

  if (userTurns.length === 0 && prompts.length === 0) {
    return null;
  }

  const averageTokensPerSecond =
    totalGenerationDurationMs > 0 && totalOutputTokens > 0
      ? totalOutputTokens / (totalGenerationDurationMs / 1000)
      : 0;
  const averageMsPerToken =
    totalOutputTokens > 0 ? totalGenerationDurationMs / totalOutputTokens : 0;

  return {
    promptCount: userTurns.length,
    totalInputTokens,
    totalOutputTokens,
    averageTokensPerSecond,
    averageMsPerToken,
    prompts,
  };
}

function mergePhaseEvent(
  activityTrace: AiActivityTraceItem[],
  event: AiPhaseEvent,
): AiActivityTraceItem[] {
  const next = [...activityTrace];
  const item: AiActivityTraceItem = {
    kind: 'phase',
    ...event,
  };
  const index = next.findIndex(
    (entry) =>
      entry.kind === 'phase' &&
      entry.phase === event.phase &&
      entry.started_ts_ms === event.started_ts_ms,
  );
  if (index >= 0) {
    next[index] = item;
  } else {
    next.push(item);
  }
  return next;
}

function mergeToolEvent(
  activityTrace: AiActivityTraceItem[],
  event: AiToolActivityEvent,
): AiActivityTraceItem[] {
  const next = [...activityTrace];
  const item: AiActivityTraceItem = {
    kind: 'tool',
    ...event,
  };
  const index = next.findIndex(
    (entry) => entry.kind === 'tool' && entry.id === event.id,
  );
  if (index >= 0) {
    next[index] = item;
  } else {
    next.push(item);
  }
  return next;
}

function mergeStatusFallback(
  activityTrace: AiActivityTraceItem[],
  update: AiStatusUpdate,
): AiActivityTraceItem[] {
  if (activityTrace.some((entry) => entry.kind === 'tool' && entry.tool === update.tool)) {
    return activityTrace;
  }

  const now = Date.now();
  const state =
    update.kind === 'complete' || update.kind === 'error'
      ? update.kind
      : 'running';
  const id = `legacy-${update.tool}`;
  const next = [...activityTrace];
  const index = next.findIndex(
    (entry) => entry.kind === 'tool' && entry.id === id,
  );
  const existing =
    index >= 0 && next[index].kind === 'tool' ? next[index] : null;
  const item: AiActivityTraceItem = {
    kind: 'tool',
    id,
    tool: update.tool,
    label: update.label,
    state,
    started_ts_ms: existing?.started_ts_ms ?? now,
    finished_ts_ms: state === 'running' ? null : now,
  };
  if (index >= 0) {
    next[index] = item;
  } else {
    next.push(item);
  }
  return next;
}

function StreamingDots() {
  return (
    <span className="inline-flex items-center gap-1 ml-0.5">
      {[0, 1, 2].map((index) => (
        <span
          key={index}
          className="ai-streaming-dot"
          style={{ animationDelay: `${index * 0.2}s` }}
        />
      ))}
    </span>
  );
}

function FallbackThinkingBlock({
  text,
  live,
}: {
  text: string;
  live: boolean;
}) {
  const [open, setOpen] = useState(live);

  useEffect(() => {
    if (live) {
      setOpen(true);
    }
  }, [live]);

  return (
    <div className="mb-3 space-y-1.5">
      <button
        type="button"
        onClick={() => setOpen((current) => !current)}
        className="ai-activity-fallback-toggle"
      >
        <span className="ai-activity-primary">
          {live ? 'Thinking' : open ? 'Hide fallback note' : 'Show fallback note'}
        </span>
        {!live ? (
          <svg
            className={`h-3 w-3 text-[var(--text-dim)] transition-transform ${open ? 'rotate-180' : ''}`}
            viewBox="0 0 16 16"
            fill="currentColor"
          >
            <path d="M8 11L2 5h12z" />
          </svg>
        ) : null}
      </button>
      {open ? (
        <div className="ai-activity-fallback-body">
          {text}
          {live ? <span className="ai-cursor" /> : null}
        </div>
      ) : null}
    </div>
  );
}

function StatsBar({ stats }: { stats: AiTurnStats }) {
  const turnDuration = stats.end_to_end_duration_ms || stats.total_duration_ms;
  const hasQueue = stats.queue_duration_ms > 0;
  const hasLoad = stats.model_load_duration_ms > 0;

  return (
    <div className="mt-2.5 flex flex-wrap gap-x-4 gap-y-1 border-t border-[var(--border)] pt-2.5 text-[0.7rem]">
      <span className="flex gap-1">
        <span className="muted">Turn</span>
        <span className="font-semibold tabular-nums muted">
          {formatMs(turnDuration)}
        </span>
      </span>
      <span className="flex gap-1">
        <span className="muted">Plan</span>
        <span className="font-semibold tabular-nums muted">
          {formatMs(stats.planner_duration_ms)}
        </span>
      </span>
      <span className="flex gap-1">
        <span className="muted">Tools</span>
        <span className="font-semibold tabular-nums muted">
          {formatMs(stats.tool_duration_ms)}
        </span>
      </span>
      <span className="flex gap-1">
        <span className="muted">Generate</span>
        <span className="font-semibold tabular-nums muted">
          {formatMs(stats.generation_duration_ms)}
        </span>
      </span>
      {hasQueue ? (
        <span className="flex gap-1">
          <span className="muted">Queue</span>
          <span className="font-semibold tabular-nums muted">
            {formatMs(stats.queue_duration_ms)}
          </span>
        </span>
      ) : null}
      {hasLoad ? (
        <span className="flex gap-1">
          <span className="muted">Load</span>
          <span className="font-semibold tabular-nums muted">
            {formatMs(stats.model_load_duration_ms)}
          </span>
        </span>
      ) : null}
      <span className="flex gap-1">
        <span className="muted">Prompt</span>
        <span className="font-semibold tabular-nums muted">
          {stats.prompt_tokens} tok
        </span>
      </span>
      <span className="flex gap-1">
        <span className="muted">Output</span>
        <span className="font-semibold tabular-nums muted">
          {stats.completion_tokens} tok
        </span>
      </span>
      <span className="flex gap-1">
        <span className="muted">Speed</span>
        <span className="font-semibold tabular-nums text-[var(--orange-soft)]">
          {formatTps(stats.tokens_per_second)}
        </span>
      </span>
    </div>
  );
}

function runtimePhaseLabel(phase: AiRuntimeResponse['turn']['phase']): string {
  switch (phase) {
    case 'loading_model':
      return 'Loading';
    case 'planning':
      return 'Planning';
    case 'grounding':
      return 'Grounding';
    case 'generating':
      return 'Generating';
    default:
      return 'Idle';
  }
}

function pendingActionExpired(pendingAction: AiPendingAction): boolean {
  return (
    pendingAction.status === 'expired' ||
    (pendingAction.status === 'pending' && pendingAction.expires_ts * 1000 < Date.now())
  );
}

function PendingActionCard({
  pendingAction,
  busy,
  disabled,
  onConfirm,
}: {
  pendingAction: AiPendingAction;
  busy: boolean;
  disabled: boolean;
  onConfirm: (pendingAction: AiPendingAction) => void;
}) {
  const expired = pendingActionExpired(pendingAction);
  const confirmed = pendingAction.status === 'confirmed';
  const statusLabel = confirmed ? 'Confirmed' : expired ? 'Expired' : 'Confirmation required';

  return (
    <div
      className="mt-3 rounded-2xl border px-4 py-3"
      style={{
        background: 'rgba(255,145,77,0.08)',
        borderColor: 'rgba(255,145,77,0.22)',
      }}
    >
      <div className="mb-2 flex items-start justify-between gap-3">
        <div>
          <p className="text-[0.68rem] font-semibold uppercase tracking-[0.14em] text-[var(--orange-soft)]">
            {statusLabel}
          </p>
          <p className="mt-1 text-xs muted">Calendar action requires an explicit confirmation.</p>
        </div>
        {!confirmed && !expired ? (
          <button
            type="button"
            onClick={() => onConfirm(pendingAction)}
            disabled={disabled || busy}
            className="btn-primary rounded-xl px-3 py-1.5 text-xs disabled:cursor-not-allowed disabled:opacity-60"
          >
            {busy ? 'Confirming…' : 'Confirm'}
          </button>
        ) : null}
      </div>
      <p className="text-sm leading-relaxed">{pendingAction.summary}</p>
    </div>
  );
}

function RuntimePanel({
  runtime,
  conversationStats,
  showDetails,
  onToggleDetails,
  className = '',
  stacked = false,
}: {
  runtime: AiRuntimeResponse | null;
  conversationStats: ConversationStatsSummary | null;
  showDetails: boolean;
  onToggleDetails: () => void;
  className?: string;
  stacked?: boolean;
}) {
  if (!runtime) {
    return (
      <div className={className}>
        <p className="text-sm muted">Loading runtime telemetry…</p>
      </div>
    );
  }
  const configuredGpuCount = runtime.model.device_indices.length;
  const configuredDevices = runtimeDeviceSummary(runtime.model.device_indices);
  const gpuSummary =
    configuredGpuCount > 0
      ? `${configuredGpuCount} GPU${configuredGpuCount === 1 ? '' : 's'} selected`
      : runtime.gpus.length > 0
        ? `${runtime.gpus.length} GPU${runtime.gpus.length === 1 ? '' : 's'} visible`
        : 'CPU mode';

  return (
    <div className={className}>
      <div className={stacked ? 'space-y-5' : 'grid gap-5 md:grid-cols-2'}>
        <section className="border-b border-[var(--border)] pb-4">
          <p className="text-[0.68rem] uppercase tracking-[0.14em] muted">Model</p>
          <p className="mt-1 text-sm font-semibold">
            {runtime.model.name ?? 'No model loaded'}
          </p>
          <div className="mt-2 space-y-1 text-xs muted">
            <p>{runtime.model.backend} backend</p>
            <p>Context {runtime.model.context_length} · {runtime.model.n_threads} threads</p>
            <p>{runtimeSplitModeLabel(runtime.model.split_mode)} · {gpuSummary}</p>
            {configuredDevices ? <p>{configuredDevices}</p> : null}
          </div>
        </section>

        <section className="border-b border-[var(--border)] pb-4">
          <p className="text-[0.68rem] uppercase tracking-[0.14em] muted">Turn</p>
          <p className="mt-1 text-sm font-semibold">
            {runtimePhaseLabel(runtime.turn.phase)}
          </p>
          <div className="mt-2 space-y-1 text-xs muted">
            <p>{runtime.turn.active_request_count} active requests</p>
            <p>Queue depth {runtime.turn.queue_depth}</p>
          </div>
        </section>

        <section className="border-b border-[var(--border)] pb-4">
          <p className="text-[0.68rem] uppercase tracking-[0.14em] muted">Resources</p>
          <div className="mt-2 space-y-1 text-xs muted">
            <p>Process RSS {runtime.resources.process_rss_human ?? '—'}</p>
            <p>CPU {formatPercent(runtime.resources.host_cpu_percent, 1)}</p>
            <p>RAM {runtimeRamSummary(runtime.resources)}</p>
          </div>
        </section>

        <section className="space-y-2">
          <p className="text-[0.68rem] uppercase tracking-[0.14em] muted">GPUs</p>
          {runtime.gpus.length > 0 ? (
            <div className="space-y-3">
              {runtime.gpus.map((gpu) => (
                <div key={`${gpu.index ?? 'gpu'}-${gpu.name}`} className="space-y-1 text-xs muted">
                  <p className="text-sm font-semibold text-[var(--text-main)]">
                    {gpu.index !== undefined && gpu.index !== null ? `GPU ${gpu.index}` : 'GPU'} · {gpu.name}
                  </p>
                  <p>Utilization {formatPercent(gpu.utilization_percent, 0)}</p>
                  <p>VRAM {gpu.vram_used_human ?? '—'} / {gpu.vram_total_human ?? '—'}</p>
                  <p>Temp {gpu.temperature_celsius ?? '—'}°C</p>
                </div>
              ))}
            </div>
          ) : (
            <p className="text-xs muted">No GPU telemetry available for this host runtime.</p>
          )}
        </section>
      </div>

      <div className="mt-5 border-t border-[var(--border)] pt-4">
        <section className="border-b border-[var(--border)] pb-4">
          <p className="text-[0.68rem] uppercase tracking-[0.14em] muted">Chat</p>
          {conversationStats ? (
            <div className="mt-2 space-y-1 text-xs muted">
              <p>{conversationStats.promptCount} prompts</p>
              <p>{conversationStats.totalInputTokens} input tokens</p>
              <p>{conversationStats.totalOutputTokens} output tokens</p>
              <p>Avg speed {formatTps(conversationStats.averageTokensPerSecond)}</p>
              <p>Avg token time {formatMs(conversationStats.averageMsPerToken)}</p>
            </div>
          ) : (
            <p className="mt-2 text-xs muted">No completed chat stats yet.</p>
          )}
        </section>

        <button
          type="button"
          onClick={onToggleDetails}
          className="mt-4 text-[0.74rem] font-medium text-[var(--text-main)] underline underline-offset-4"
        >
          {showDetails ? 'Hide AI prompt details' : 'Show AI prompt details'}
        </button>
      </div>
    </div>
  );
}

function collectBrowserTranscript(event: BrowserSpeechRecognitionEvent): string {
  const transcripts: string[] = [];
  for (let index = 0; index < event.results.length; index += 1) {
    const result = event.results[index];
    const transcript = result?.[0]?.transcript?.trim();
    if (transcript) {
      transcripts.push(transcript);
    }
  }
  return transcripts.join(' ').trim();
}

function preferredRecorderMimeType(): string | undefined {
  if (typeof MediaRecorder === 'undefined' || typeof MediaRecorder.isTypeSupported !== 'function') {
    return undefined;
  }
  for (const mimeType of [
    'audio/webm;codecs=opus',
    'audio/webm',
    'audio/ogg;codecs=opus',
    'audio/mp4',
  ]) {
    if (MediaRecorder.isTypeSupported(mimeType)) {
      return mimeType;
    }
  }
  return undefined;
}

function MessageBubble({
  entry,
  showStats,
  onConfirmPendingAction,
  confirmingToken,
  interactionDisabled,
}: {
  entry: UiConversationTurn;
  showStats: boolean;
  onConfirmPendingAction: (pendingAction: AiPendingAction) => void;
  confirmingToken: string | null;
  interactionDisabled: boolean;
}) {
  const { thinking, content } = parseContent(entry.content);

  if (entry.role === 'user') {
    return (
      <div className="flex justify-end">
        <div
          className="max-w-[78%] rounded-2xl rounded-br-sm px-4 py-2.5 text-sm leading-relaxed whitespace-pre-wrap"
          style={{
            background: 'linear-gradient(135deg, rgba(255,145,77,0.16), rgba(157,116,255,0.16))',
            border: '1px solid rgba(255,145,77,0.26)',
          }}
        >
          {entry.content}
        </div>
      </div>
    );
  }

  const showFallbackThinking =
    Boolean(thinking) && entry.activity_trace.length === 0;

  return (
    <div className="flex justify-start">
      <div className="max-w-[84%] w-full">
        <AiAssistantActivity
          activityTrace={entry.activity_trace}
          isStreaming={entry.isStreaming}
        />

        {showFallbackThinking && thinking ? (
          <FallbackThinkingBlock
            text={thinking}
            live={entry.isStreaming && content.length === 0}
          />
        ) : null}

        <div className="panel-soft rounded-2xl rounded-bl-sm px-4 py-3 text-sm leading-relaxed">
          {entry.errorMessage ? (
            <span className="text-[var(--danger)]">{entry.errorMessage}</span>
          ) : content ? (
            <span className="whitespace-pre-wrap">{content}</span>
          ) : entry.isStreaming ? (
            <StreamingDots />
          ) : null}

          {entry.isStreaming && content ? <span className="ai-cursor" /> : null}
        </div>

        {showStats && entry.stats ? <StatsBar stats={entry.stats} /> : null}

        {entry.pending_action ? (
          <PendingActionCard
            pendingAction={entry.pending_action}
            busy={confirmingToken === entry.pending_action.token}
            disabled={interactionDisabled}
            onConfirm={onConfirmPendingAction}
          />
        ) : null}
      </div>
    </div>
  );
}

function InferenceUnavailable({
  serviceUnavailable = false,
  title,
  description,
  showRecommendation,
}: {
  serviceUnavailable?: boolean;
  title?: string;
  description?: string;
  showRecommendation?: boolean;
}) {
  const resolvedTitle =
    title ?? (serviceUnavailable ? 'AI unavailable on this host' : 'No model installed');
  const resolvedDescription =
    description ??
    (serviceUnavailable
      ? 'This host is running without an enabled AI inference backend, so the assistant is unavailable right now.'
      : 'No AI models are installed right now. Ask an admin to manage models from Admin > AI.');
  const shouldShowRecommendation = showRecommendation ?? !serviceUnavailable;

  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-5 px-6 py-16 text-center">
      <div
        className="flex h-14 w-14 items-center justify-center rounded-2xl"
        style={{
          background: 'rgba(157,116,255,0.12)',
          border: '1px solid rgba(157,116,255,0.22)',
        }}
      >
        <svg
          className="h-7 w-7 text-[var(--purple)]"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.8"
        >
          <path d="M12 3v18M3 12h18" />
        </svg>
      </div>
      <div className="space-y-1.5">
        <p className="font-semibold">{resolvedTitle}</p>
        <p className="max-w-xs text-sm muted">{resolvedDescription}</p>
      </div>
      {shouldShowRecommendation ? (
        <p className="text-xs muted">Starter installs use Qwen2.5 1.5B, roughly 1 GB on disk.</p>
      ) : null}
    </div>
  );
}

function EmptyState({
  model,
  isAdmin,
  hasConversation,
  suggestionKey,
  onNewChat,
  onSuggest,
}: {
  model: string;
  isAdmin: boolean;
  hasConversation: boolean;
  suggestionKey: string;
  onNewChat: () => void;
  onSuggest: (value: string) => void;
}) {
  const [suggestions, setSuggestions] = useState<string[]>(() =>
    pickStarterSuggestions(isAdmin),
  );

  useEffect(() => {
    setSuggestions(pickStarterSuggestions(isAdmin));
  }, [isAdmin, suggestionKey]);

  return (
    <div className="flex h-full flex-col items-center justify-center gap-6 px-4 py-8 text-center">
      <div>
        <div
          className="mx-auto mb-4 flex h-12 w-12 items-center justify-center rounded-2xl"
          style={{
            background: 'linear-gradient(135deg, rgba(255,145,77,0.18), rgba(157,116,255,0.18))',
            border: '1px solid rgba(177,140,255,0.22)',
          }}
        >
          <svg
            className="h-6 w-6"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.6"
          >
            <defs>
              <linearGradient id="ai-icon-grad" x1="0" y1="0" x2="1" y2="1">
                <stop offset="0%" stopColor="var(--orange)" />
                <stop offset="100%" stopColor="var(--purple)" />
              </linearGradient>
            </defs>
            <path stroke="url(#ai-icon-grad)" d="M9.5 3.5A6.5 6.5 0 1 1 3 10m0 4a6.5 6.5 0 0 0 11.949 3.5" />
            <circle cx="12" cy="12" r="2.5" stroke="url(#ai-icon-grad)" />
          </svg>
        </div>
        <p className="font-semibold text-base">
          Ask <span className="accent-logo">{modelDisplayName(model)}</span> anything
        </p>
        <p className="mt-1 max-w-md text-sm muted">
          {hasConversation
            ? 'This conversation is ready. Ask for grounded calendar, library, room, network, runtime, or download details.'
            : 'Create a stored conversation, then ask grounded questions about your calendar, libraries, rooms, downloads, or host state.'}
        </p>
      </div>

      {!hasConversation ? (
        <button
          type="button"
          onClick={onNewChat}
          className="btn-primary rounded-xl px-5 py-3 text-sm"
        >
          Start a new chat
        </button>
      ) : null}

      <div className="grid w-full max-w-2xl grid-cols-1 gap-2 sm:grid-cols-2">
        {suggestions.map((suggestion) => (
          <button
            key={suggestion}
            type="button"
            onClick={() => onSuggest(suggestion)}
            className="rounded-xl px-3.5 py-2.5 text-left text-sm transition-all duration-150"
            style={{
              background: 'rgba(255,255,255,0.04)',
              border: '1px solid var(--border)',
            }}
            onMouseEnter={(event) => {
              event.currentTarget.style.borderColor = 'var(--border-strong)';
            }}
            onMouseLeave={(event) => {
              event.currentTarget.style.borderColor = 'var(--border)';
            }}
          >
            {suggestion}
          </button>
        ))}
      </div>
    </div>
  );
}

function ModelSelector({
  models,
  selected,
  onChange,
}: {
  models: AiModel[];
  selected: string;
  onChange: (value: string) => void;
}) {
  return (
    <div className="relative w-full sm:w-auto">
      <select
        value={selected}
        onChange={(event) => onChange(event.target.value)}
        className="ai-model-select w-full appearance-none cursor-pointer rounded-xl border border-[var(--border)] bg-[var(--surface)] py-1.5 pl-3 pr-8 text-sm text-[var(--text-main)] transition-colors"
      >
        {models.map((model) => (
          <option key={model.name} value={model.name}>
            {modelDisplayName(model.name)}
            {model.parameter_size ? ` (${model.parameter_size})` : ''}
          </option>
        ))}
      </select>
      <svg
        className="pointer-events-none absolute right-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 muted"
        viewBox="0 0 16 16"
        fill="currentColor"
      >
        <path d="M8 11L2 5h12z" />
      </svg>
    </div>
  );
}

export default function AiPage() {
  const router = useRouter();
  const { me, loading: authLoading } = useAuth();

  const [models, setModels] = useState<AiModel[]>([]);
  const [inferenceAvailable, setInferenceAvailable] = useState<boolean | null>(null);
  const [serviceUnavailable, setServiceUnavailable] = useState(false);
  const [modelStorageAvailable, setModelStorageAvailable] = useState(true);
  const [modelsError, setModelsError] = useState<string | null>(null);
  const [selectedModel, setSelectedModel] = useState('');

  const [conversations, setConversations] = useState<AiConversationSummary[]>([]);
  const [conversationDetails, setConversationDetails] = useState<Record<string, UiConversationDetail>>({});
  const [activeConversationId, setActiveConversationId] = useState<string | null>(null);
  const [queuedPrompts, setQueuedPrompts] = useState<QueuedPromptMap>({});
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [runtimeDrawerOpen, setRuntimeDrawerOpen] = useState(false);
  const [desktopRuntimeOpen, setDesktopRuntimeOpen] = useState(false);
  const [desktopRailOpen, setDesktopRailOpen] = useState(true);
  const [conversationError, setConversationError] = useState('');
  const [conversationsLoading, setConversationsLoading] = useState(false);
  const [loadingConversationId, setLoadingConversationId] = useState<string | null>(null);

  const [input, setInput] = useState('');
  const [isStreaming, setIsStreaming] = useState(false);
  const [streamingConversationId, setStreamingConversationId] = useState<string | null>(null);
  const [confirmingToken, setConfirmingToken] = useState<string | null>(null);
  const [autoSendRequest, setAutoSendRequest] = useState<{
    conversationId: string;
    text: string;
  } | null>(null);

  const [runtime, setRuntime] = useState<AiRuntimeResponse | null>(null);
  const [runtimeError, setRuntimeError] = useState<string | null>(null);

  const [voiceState, setVoiceState] = useState<VoiceState>('idle');
  const [voiceError, setVoiceError] = useState<string | null>(null);
  const [showPromptDetails, setShowPromptDetails] = useState(false);

  const [renameTarget, setRenameTarget] = useState<AiConversationSummary | null>(null);
  const [renameValue, setRenameValue] = useState('');
  const [deleteTarget, setDeleteTarget] = useState<AiConversationSummary | null>(null);
  const [modalBusy, setModalBusy] = useState(false);

  const queuedPromptsRef = useRef<QueuedPromptMap>({});
  const activeConversationIdRef = useRef<string | null>(null);
  const messageScrollRef = useRef<HTMLDivElement | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  const stopRef = useRef<(() => void) | null>(null);
  const recognitionRef = useRef<BrowserSpeechRecognition | null>(null);
  const mediaRecorderRef = useRef<MediaRecorder | null>(null);
  const mediaStreamRef = useRef<MediaStream | null>(null);
  const mediaChunksRef = useRef<Blob[]>([]);
  const autoStickToBottomRef = useRef(true);
  const stopIntentRef = useRef<{
    conversationId: string;
    queuedText: string | null;
  } | null>(null);

  const activeConversation = activeConversationId
    ? conversationDetails[activeConversationId] ?? null
    : null;
  const lastActiveMessageContent =
    activeConversation && activeConversation.messages.length > 0
      ? activeConversation.messages[activeConversation.messages.length - 1].content
      : '';
  const archivedConversations = conversations.filter((conversation) => conversation.archived);
  const liveConversations = conversations.filter((conversation) => !conversation.archived);
  const activeConversationStats = buildConversationStatsSummary(activeConversation);
  const streamingElsewhere =
    Boolean(streamingConversationId) && streamingConversationId !== activeConversationId;
  const queuedPrompt =
    activeConversationId ? queuedPrompts[activeConversationId] ?? '' : '';
  const hasQueuedPrompt = queuedPrompt.trim().length > 0;

  const resetComposerHeight = useCallback(() => {
    if (textareaRef.current) {
      textareaRef.current.style.height = 'auto';
    }
  }, []);

  const focusComposer = useCallback(() => {
    textareaRef.current?.focus();
  }, []);

  const scrollMessagesToBottom = useCallback((behavior: ScrollBehavior = 'auto') => {
    const node = messageScrollRef.current;
    if (!node) return;
    node.scrollTo({ top: node.scrollHeight, behavior });
  }, []);

  const handleMessageScroll = useCallback(() => {
    const node = messageScrollRef.current;
    if (!node) return;
    const distanceFromBottom = node.scrollHeight - (node.scrollTop + node.clientHeight);
    autoStickToBottomRef.current = distanceFromBottom <= 48;
  }, []);

  const upsertQueuedPrompt = useCallback((conversationId: string, text: string) => {
    const normalized = text.trim();
    setQueuedPrompts((current) => {
      const next = { ...current };
      if (normalized) {
        next[conversationId] = normalized;
      } else {
        delete next[conversationId];
      }
      queuedPromptsRef.current = next;
      return next;
    });
  }, []);

  const clearQueuedPrompt = useCallback((conversationId: string) => {
    setQueuedPrompts((current) => {
      if (!(conversationId in current)) {
        return current;
      }
      const next = { ...current };
      delete next[conversationId];
      queuedPromptsRef.current = next;
      return next;
    });
  }, []);

  const storeConversationDetail = useCallback((detail: AiConversationDetail) => {
    const nextConversation = toUiConversation(detail);
    setConversationDetails((current) => ({
      ...current,
      [nextConversation.id]: nextConversation,
    }));
    setConversations((current) =>
      upsertConversationSummary(current, buildConversationSummary(nextConversation)),
    );
  }, []);

  const loadModels = useCallback(async () => {
    try {
      const response = await fetchModels();
      setInferenceAvailable(response.inference_available);
      setServiceUnavailable(response.service_unavailable);
      setModelStorageAvailable(response.model_storage_available);
      setModelsError(response.model_storage_error);
      setModels(response.models);
      setSelectedModel((current) => {
        if (response.models.length === 0) return '';
        if (current && response.models.some((model) => model.name === current)) {
          return current;
        }
        return response.models[0].name;
      });
    } catch (error) {
      setInferenceAvailable(false);
      setServiceUnavailable(false);
      setModelStorageAvailable(false);
      setModels([]);
      setSelectedModel('');
      setModelsError(
        clientErrorMessage(
          error,
          'Failed to connect to the Rustyfin backend. Check that the native runtime is online.',
        ),
      );
    }
  }, []);

  const loadRuntime = useCallback(async () => {
    try {
      const nextRuntime = await fetchAiRuntime();
      setRuntime(nextRuntime);
      setRuntimeError(null);
    } catch (error) {
      setRuntimeError(clientErrorMessage(error, 'Failed to load AI runtime status.'));
    }
  }, []);

  const loadConversationList = useCallback(
    async (preferredConversationId?: string | null) => {
      setConversationsLoading(true);
      try {
        const nextConversations = sortConversationSummaries(
          await listConversations(true),
        );
        setConversationError('');
        setConversations(nextConversations);
        setActiveConversationId((current) =>
          chooseConversationId(nextConversations, preferredConversationId ?? current),
        );
      } catch (error) {
        setConversationError(
          clientErrorMessage(error, 'Failed to load AI conversations.'),
        );
      } finally {
        setConversationsLoading(false);
      }
    },
    [],
  );

  const loadConversationDetail = useCallback(
    async (conversationId: string) => {
      setLoadingConversationId(conversationId);
      try {
        const detail = await getConversation(conversationId);
        setConversationError('');
        storeConversationDetail(detail);
      } catch (error) {
        setConversationError(
          clientErrorMessage(error, 'Failed to load this conversation.'),
        );
      } finally {
        setLoadingConversationId((current) =>
          current === conversationId ? null : current,
        );
      }
    },
    [storeConversationDetail],
  );

  const createConversationRecord = useCallback(async () => {
    const detail = await createConversation();
    storeConversationDetail(detail);
    setConversationError('');
    setActiveConversationId(detail.id);
    setDrawerOpen(false);
    return detail;
  }, [storeConversationDetail]);

  useEffect(() => {
    queuedPromptsRef.current = queuedPrompts;
  }, [queuedPrompts]);

  useEffect(() => {
    activeConversationIdRef.current = activeConversationId;
  }, [activeConversationId]);

  useEffect(() => {
    if (!authLoading && !me) {
      router.replace('/login');
    }
  }, [authLoading, me, router]);

  useEffect(() => {
    if (!me) return;
    void loadModels();
    void loadConversationList();
    void loadRuntime();
  }, [loadConversationList, loadModels, loadRuntime, me]);

  useEffect(() => {
    if (!me || inferenceAvailable !== true || serviceUnavailable) return;
    const intervalMs = isStreaming ? 2000 : 10000;
    const intervalId = window.setInterval(() => {
      void loadRuntime();
    }, intervalMs);
    return () => window.clearInterval(intervalId);
  }, [inferenceAvailable, isStreaming, loadRuntime, me, serviceUnavailable]);

  useEffect(() => {
    if (!me || !activeConversationId) return;
    void loadConversationDetail(activeConversationId);
  }, [activeConversationId, loadConversationDetail, me]);

  useEffect(() => {
    if (typeof document === 'undefined') return;
    document.documentElement.dataset.rfPage = 'ai';
    document.body.dataset.rfPage = 'ai';
    return () => {
      delete document.documentElement.dataset.rfPage;
      delete document.body.dataset.rfPage;
    };
  }, []);

  useEffect(() => {
    if (!autoStickToBottomRef.current) return;
    window.requestAnimationFrame(() => {
      scrollMessagesToBottom('auto');
    });
  }, [
    activeConversationId,
    activeConversation?.messages.length,
    lastActiveMessageContent,
    scrollMessagesToBottom,
  ]);

  useEffect(() => {
    autoStickToBottomRef.current = true;
    window.requestAnimationFrame(() => {
      scrollMessagesToBottom('auto');
    });
  }, [activeConversationId, scrollMessagesToBottom]);

  useEffect(() => {
    if (!drawerOpen && !runtimeDrawerOpen) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        setDrawerOpen(false);
        setRuntimeDrawerOpen(false);
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [drawerOpen, runtimeDrawerOpen]);

  useEffect(() => {
    if (inferenceAvailable === true && selectedModel) return;
    setRuntimeDrawerOpen(false);
    setDesktopRuntimeOpen(false);
  }, [inferenceAvailable, selectedModel]);

  useEffect(() => {
    return () => {
      stopRef.current?.();
      recognitionRef.current?.abort();
      if (mediaRecorderRef.current?.state && mediaRecorderRef.current.state !== 'inactive') {
        mediaRecorderRef.current.stop();
      }
      mediaStreamRef.current?.getTracks().forEach((track) => track.stop());
    };
  }, []);

  const updateAssistantTurn = useCallback(
    (
      conversationId: string,
      assistantTurnId: string,
      updater: (turn: UiConversationTurn) => UiConversationTurn,
    ) => {
      setConversationDetails((current) => {
        const conversation = current[conversationId];
        if (!conversation) return current;
        return {
          ...current,
          [conversationId]: {
            ...conversation,
            messages: conversation.messages.map((turn) =>
              turn.id === assistantTurnId ? updater(turn) : turn,
            ),
          },
        };
      });
    },
    [],
  );

  const finalizeAssistantTurn = useCallback(
    (conversationId: string, assistantTurnId: string) => {
      updateAssistantTurn(conversationId, assistantTurnId, (turn) => ({
        ...turn,
        isStreaming: false,
      }));
    },
    [updateAssistantTurn],
  );

  const applyVoiceTranscript = useCallback(
    (transcript: string) => {
      const next = transcript.trim();
      if (!next) {
        setVoiceState('error');
        setVoiceError('Rustyfin could not detect any speech in that recording.');
        return;
      }
      setInput(next);
      setVoiceState('idle');
      setVoiceError(null);
      requestAnimationFrame(() => {
        focusComposer();
        if (textareaRef.current) {
          textareaRef.current.style.height = 'auto';
          textareaRef.current.style.height = `${Math.min(textareaRef.current.scrollHeight, 160)}px`;
        }
      });
    },
    [focusComposer],
  );

  const releaseVoiceCapture = useCallback(() => {
    mediaRecorderRef.current = null;
    mediaChunksRef.current = [];
    mediaStreamRef.current?.getTracks().forEach((track) => track.stop());
    mediaStreamRef.current = null;
  }, []);

  const handleInputChange = (event: React.ChangeEvent<HTMLTextAreaElement>) => {
    const nextValue = event.target.value;
    setInput(nextValue);
    const queuedConversationId = activeConversationIdRef.current;
    if (queuedConversationId && queuedConversationId in queuedPromptsRef.current) {
      upsertQueuedPrompt(queuedConversationId, nextValue);
    }
    event.target.style.height = 'auto';
    event.target.style.height = `${Math.min(event.target.scrollHeight, 160)}px`;
  };

  const handleSuggestion = useCallback(
    (value: string) => {
      setInput(value);
      const queuedConversationId = activeConversationIdRef.current;
      if (queuedConversationId && queuedConversationId in queuedPromptsRef.current) {
        upsertQueuedPrompt(queuedConversationId, value);
      }
      requestAnimationFrame(() => {
        focusComposer();
        if (textareaRef.current) {
          textareaRef.current.style.height = 'auto';
          textareaRef.current.style.height = `${Math.min(textareaRef.current.scrollHeight, 160)}px`;
        }
      });
    },
    [focusComposer, upsertQueuedPrompt],
  );

  const handleSelectConversation = useCallback(
    (conversationId: string) => {
      setConversationError('');
      setActiveConversationId(conversationId);
      setDrawerOpen(false);
      setInput(queuedPromptsRef.current[conversationId] ?? '');
      requestAnimationFrame(() => {
        resetComposerHeight();
        if (textareaRef.current) {
          textareaRef.current.style.height = 'auto';
          textareaRef.current.style.height = `${Math.min(textareaRef.current.scrollHeight, 160)}px`;
        }
      });
    },
    [resetComposerHeight],
  );

  const handleNewChat = useCallback(async () => {
    try {
      setConversationError('');
      setInput('');
      resetComposerHeight();
      await createConversationRecord();
      requestAnimationFrame(() => focusComposer());
    } catch (error) {
      setConversationError(
        clientErrorMessage(error, 'Failed to create a new conversation.'),
      );
    }
  }, [createConversationRecord, focusComposer, resetComposerHeight]);

  const handleArchiveToggle = useCallback(
    async (conversation: AiConversationSummary) => {
      try {
        setConversationError('');
        const detail = await updateConversation(conversation.id, {
          archived: !conversation.archived,
        });
        storeConversationDetail(detail);
      } catch (error) {
        setConversationError(
          clientErrorMessage(error, 'Failed to update this conversation.'),
        );
      }
    },
    [storeConversationDetail],
  );

  const handleRenameConversation = useCallback((conversation: AiConversationSummary) => {
    setRenameTarget(conversation);
    setRenameValue(conversation.title);
  }, []);

  const confirmRenameConversation = useCallback(async () => {
    if (!renameTarget) return;
    const title = renameValue.trim();
    if (!title) {
      setConversationError('Conversation title cannot be empty.');
      return;
    }

    setModalBusy(true);
    try {
      setConversationError('');
      const detail = await updateConversation(renameTarget.id, { title });
      storeConversationDetail(detail);
      setRenameTarget(null);
      setRenameValue('');
    } catch (error) {
      setConversationError(
        clientErrorMessage(error, 'Failed to rename this conversation.'),
      );
    } finally {
      setModalBusy(false);
    }
  }, [renameTarget, renameValue, storeConversationDetail]);

  const handleDeleteConversation = useCallback((conversation: AiConversationSummary) => {
    if (isStreaming && streamingConversationId === conversation.id) {
      setConversationError('Stop the active response before deleting this conversation.');
      return;
    }
    setDeleteTarget(conversation);
  }, [isStreaming, streamingConversationId]);

  const confirmDeleteConversation = useCallback(async () => {
    if (!deleteTarget) return;

    setModalBusy(true);
    try {
      setConversationError('');
      const targetElement = findDataDeleteTarget(
        'data-ai-conversation-row-id',
        deleteTarget.id,
      );
      await playTelegramDeleteAnimation(targetElement);
      await deleteConversation(deleteTarget.id);

      const remainingConversations = conversations.filter(
        (conversation) => conversation.id !== deleteTarget.id,
      );
      const nextActiveId = chooseConversationId(remainingConversations, activeConversationId === deleteTarget.id ? null : activeConversationId);

      setConversations(remainingConversations);
      setConversationDetails((current) => {
        const next = { ...current };
        delete next[deleteTarget.id];
        return next;
      });
      clearQueuedPrompt(deleteTarget.id);
      if (activeConversationId === deleteTarget.id) {
        setInput('');
        resetComposerHeight();
      }
      setActiveConversationId(nextActiveId);
      setDeleteTarget(null);
    } catch (error) {
      setConversationError(
        clientErrorMessage(error, 'Failed to delete this conversation.'),
      );
    } finally {
      setModalBusy(false);
    }
  }, [activeConversationId, clearQueuedPrompt, conversations, deleteTarget, resetComposerHeight]);

  const startVoiceInput = useCallback(async () => {
    if (voiceState === 'recording' || voiceState === 'stopping' || voiceState === 'transcribing') {
      return;
    }

    setVoiceError(null);

    const SpeechRecognitionCtor =
      typeof window !== 'undefined'
        ? window.SpeechRecognition ?? window.webkitSpeechRecognition
        : undefined;

    if (SpeechRecognitionCtor) {
      let latestTranscript = '';
      const recognition = new SpeechRecognitionCtor();
      recognitionRef.current = recognition;
      recognition.lang = 'en-US';
      recognition.continuous = false;
      recognition.interimResults = true;
      recognition.maxAlternatives = 1;
      recognition.onresult = (event) => {
        latestTranscript = collectBrowserTranscript(event);
        if (latestTranscript) {
          setInput(latestTranscript);
          requestAnimationFrame(() => {
            if (textareaRef.current) {
              textareaRef.current.style.height = 'auto';
              textareaRef.current.style.height = `${Math.min(textareaRef.current.scrollHeight, 160)}px`;
            }
          });
        }
      };
      recognition.onerror = (event) => {
        recognitionRef.current = null;
        setVoiceState('error');
        setVoiceError(
          event.error === 'not-allowed'
            ? 'Microphone permission was denied.'
            : 'Browser voice recognition failed.',
        );
      };
      recognition.onend = () => {
        recognitionRef.current = null;
        if (latestTranscript.trim()) {
          applyVoiceTranscript(latestTranscript);
          return;
        }
        setVoiceState('idle');
      };
      setVoiceState('recording');
      recognition.start();
      return;
    }

    if (!navigator.mediaDevices?.getUserMedia || typeof MediaRecorder === 'undefined') {
      setVoiceState('error');
      setVoiceError('This browser does not support AI voice input.');
      return;
    }

    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      const mimeType = preferredRecorderMimeType();
      const recorder = mimeType ? new MediaRecorder(stream, { mimeType }) : new MediaRecorder(stream);
      mediaStreamRef.current = stream;
      mediaRecorderRef.current = recorder;
      mediaChunksRef.current = [];

      recorder.ondataavailable = (event) => {
        if (event.data.size > 0) {
          mediaChunksRef.current.push(event.data);
        }
      };
      recorder.onerror = () => {
        releaseVoiceCapture();
        setVoiceState('error');
        setVoiceError('Audio recording failed.');
      };
      recorder.onstop = () => {
        const chunks = [...mediaChunksRef.current];
        releaseVoiceCapture();
        void (async () => {
          try {
            setVoiceState('transcribing');
            const blob = new Blob(chunks, {
              type: recorder.mimeType || mimeType || 'audio/webm',
            });
            const response = await transcribeAiInput(blob);
            applyVoiceTranscript(response.text);
          } catch (error) {
            setVoiceState('error');
            setVoiceError(
              clientErrorMessage(error, 'Rustyfin could not transcribe that recording.'),
            );
          }
        })();
      };

      setVoiceState('recording');
      recorder.start();
    } catch (error) {
      releaseVoiceCapture();
      setVoiceState('error');
      setVoiceError(clientErrorMessage(error, 'Failed to access the microphone.'));
    }
  }, [applyVoiceTranscript, releaseVoiceCapture, voiceState]);

  const stopVoiceInput = useCallback(() => {
    if (voiceState !== 'recording') return;
    setVoiceState('stopping');
    if (recognitionRef.current) {
      recognitionRef.current.stop();
      return;
    }
    if (mediaRecorderRef.current && mediaRecorderRef.current.state !== 'inactive') {
      mediaRecorderRef.current.stop();
      return;
    }
    setVoiceState('idle');
  }, [voiceState]);

  const sendMessage = useCallback(async (options?: {
    text?: string;
    confirmationToken?: string;
    conversationIdOverride?: string;
    bypassQueue?: boolean;
  }) => {
    const confirmationToken = options?.confirmationToken;
    const text = (options?.text ?? input).trim();
    const conversationIdOverride = options?.conversationIdOverride ?? activeConversationId;
    const bypassQueue = options?.bypassQueue === true;
    const canQueueFollowUp =
      !confirmationToken &&
      !bypassQueue &&
      isStreaming &&
      Boolean(conversationIdOverride) &&
      conversationIdOverride === streamingConversationId;

    if (!text || !selectedModel) return;
    if (inferenceAvailable !== true || !modelStorageAvailable) return;
    if (canQueueFollowUp && conversationIdOverride) {
      setConversationError('');
      upsertQueuedPrompt(conversationIdOverride, text);
      setVoiceState('idle');
      setVoiceError(null);
      return;
    }
    if (isStreaming) return;
    if (activeConversation?.archived) {
      setConversationError('Restore this archived conversation before sending a new message.');
      return;
    }

    let conversationId = conversationIdOverride;
    let conversationTitle = activeConversation?.title ?? DEFAULT_CONVERSATION_TITLE;

    try {
      setConversationError('');
      autoStickToBottomRef.current = true;
      if (!conversationId) {
        const detail = await createConversationRecord();
        conversationId = detail.id;
        conversationTitle = detail.title;
      }

      if (!conversationId) {
        throw new Error('No conversation is available for this message.');
      }

      const savedQueuedPrompt = queuedPromptsRef.current[conversationId]?.trim();
      if ((bypassQueue || savedQueuedPrompt === text) && savedQueuedPrompt) {
        clearQueuedPrompt(conversationId);
        if (
          activeConversationIdRef.current === conversationId &&
          input.trim() === text
        ) {
          setInput('');
          resetComposerHeight();
        }
      }

      const sentTs = nowTsSeconds();
      const userTurnId = uid('local-user');
      const assistantTurnId = uid('local-assistant');
      const preview = normalizePreview(text);
      const nextTitle =
        conversationTitle === DEFAULT_CONVERSATION_TITLE
          ? suggestedConversationTitle(text)
          : conversationTitle;

      const userTurn: UiConversationTurn = {
        id: userTurnId,
        role: 'user',
        content: text,
        model_name: null,
        grounding_tools: [],
        follow_up_contexts: [],
        grounding_sources: [],
        activity_trace: [],
        stats: null,
        created_ts: sentTs,
        isStreaming: false,
        errorMessage: null,
      };

      const assistantTurn: UiConversationTurn = {
        id: assistantTurnId,
        role: 'assistant',
        content: '',
        model_name: selectedModel,
        grounding_tools: [],
        follow_up_contexts: [],
        grounding_sources: [],
        activity_trace: [],
        stats: null,
        created_ts: sentTs,
        isStreaming: true,
        errorMessage: null,
      };

      setConversationDetails((current) => {
        const conversation = current[conversationId!];
        if (!conversation) return current;
        return {
          ...current,
          [conversationId!]: {
            ...conversation,
            title: nextTitle,
            archived: false,
            last_message_preview: preview,
            last_model_name: selectedModel,
            updated_ts: sentTs,
            messages: [...conversation.messages, userTurn, assistantTurn],
          },
        };
      });
      setConversations((current) =>
        upsertConversationSummary(current, {
          id: conversationId!,
          title: nextTitle,
          last_message_preview: preview,
          last_model_name: selectedModel,
          updated_ts: sentTs,
          archived: false,
        }),
      );

      if (!confirmationToken) {
        setInput('');
        resetComposerHeight();
      }
      setIsStreaming(true);
      setStreamingConversationId(conversationId);
      setConfirmingToken(confirmationToken ?? null);
      setDrawerOpen(false);
      setVoiceState('idle');
      setVoiceError(null);

      let completed = false;
      let latestAssistantContent = '';
      let requiresConfirmation = false;
      let streamFailed = false;

      stopRef.current = streamConversationMessage(
        conversationId,
        selectedModel,
        text,
        confirmationToken,
        (event) => {
          if (event.type === 'phase') {
            updateAssistantTurn(conversationId!, assistantTurnId, (turn) => ({
              ...turn,
              activity_trace: mergePhaseEvent(turn.activity_trace, event.phase),
            }));
            return;
          }

          if (event.type === 'tool') {
            updateAssistantTurn(conversationId!, assistantTurnId, (turn) => ({
              ...turn,
              activity_trace: mergeToolEvent(turn.activity_trace, event.activity),
            }));
            return;
          }

          if (event.type === 'status') {
            updateAssistantTurn(conversationId!, assistantTurnId, (turn) => ({
              ...turn,
              activity_trace: mergeStatusFallback(turn.activity_trace, event.update),
            }));
            return;
          }

          if (event.type === 'confirmation_required') {
            requiresConfirmation = true;
            updateAssistantTurn(conversationId!, assistantTurnId, (turn) => ({
              ...turn,
              pending_action: {
                ...event.confirmation,
                status: 'pending',
              },
            }));
            return;
          }

          if (event.type === 'token') {
            latestAssistantContent += event.text;
            updateAssistantTurn(conversationId!, assistantTurnId, (turn) => ({
              ...turn,
              content: turn.content + event.text,
              errorMessage: null,
            }));
            return;
          }

          if (event.type === 'grounding') {
            updateAssistantTurn(conversationId!, assistantTurnId, (turn) => ({
              ...turn,
              grounding_sources: event.sources,
              follow_up_contexts: event.followUpContexts,
              grounding_tools: event.sources.map((source) => source.tool),
            }));
            return;
          }

          if (event.type === 'stats') {
            updateAssistantTurn(conversationId!, assistantTurnId, (turn) => ({
              ...turn,
              stats: event.stats,
            }));
            return;
          }

          if (event.type === 'error') {
            streamFailed = true;
            updateAssistantTurn(conversationId!, assistantTurnId, (turn) => ({
              ...turn,
              isStreaming: false,
              errorMessage: event.message,
            }));
            return;
          }

          if (event.type === 'done') {
            completed = true;
            finalizeAssistantTurn(conversationId!, assistantTurnId);
            if (latestAssistantContent.trim()) {
              const finishedTs = nowTsSeconds();
              setConversations((current) =>
                upsertConversationSummary(current, {
                  id: conversationId!,
                  title: nextTitle,
                  last_message_preview: normalizePreview(latestAssistantContent),
                  last_model_name: selectedModel,
                  updated_ts: finishedTs,
                  archived: false,
                }),
              );
            }
          }
        },
        () => {
          stopRef.current = null;
          setIsStreaming(false);
          setStreamingConversationId(null);
          setConfirmingToken(null);
          finalizeAssistantTurn(conversationId!, assistantTurnId);
          void loadRuntime();

          const stopIntent = stopIntentRef.current;
          if (stopIntent?.conversationId === conversationId) {
            stopIntentRef.current = null;
            if (stopIntent.queuedText) {
              setAutoSendRequest({
                conversationId: conversationId!,
                text: stopIntent.queuedText,
              });
            }
            return;
          }

          if (completed) {
            void loadConversationDetail(conversationId!);
            const queuedText = queuedPromptsRef.current[conversationId!]?.trim();
            if (queuedText && !requiresConfirmation && !streamFailed) {
              setAutoSendRequest({
                conversationId: conversationId!,
                text: queuedText,
              });
            }
          }
        },
      );
    } catch (error) {
      setConversationError(
        clientErrorMessage(error, 'Failed to send this message.'),
      );
    }
  }, [
    activeConversation?.archived,
    activeConversation?.title,
    activeConversationId,
    createConversationRecord,
    finalizeAssistantTurn,
    inferenceAvailable,
    input,
    isStreaming,
    loadConversationDetail,
    loadRuntime,
    modelStorageAvailable,
    clearQueuedPrompt,
    resetComposerHeight,
    selectedModel,
    streamingConversationId,
    updateAssistantTurn,
    upsertQueuedPrompt,
  ]);

  const handleConfirmPendingAction = useCallback(
    async (pendingAction: AiPendingAction) => {
      if (pendingActionExpired(pendingAction) || pendingAction.status === 'confirmed') {
        return;
      }
      try {
        await sendMessage({
          text: 'Confirm',
          confirmationToken: pendingAction.token,
        });
      } catch (error) {
        setConversationError(
          clientErrorMessage(error, 'Failed to confirm that calendar action.'),
        );
      }
    },
    [sendMessage],
  );

  useEffect(() => {
    if (!autoSendRequest || isStreaming) return;
    const { conversationId, text } = autoSendRequest;
    setAutoSendRequest(null);
    void sendMessage({
      text,
      conversationIdOverride: conversationId,
      bypassQueue: true,
    });
  }, [autoSendRequest, isStreaming, sendMessage]);

  const handleStop = useCallback(() => {
    if (streamingConversationId) {
      const queuedText = queuedPromptsRef.current[streamingConversationId]?.trim() || null;
      stopIntentRef.current = {
        conversationId: streamingConversationId,
        queuedText,
      };
    } else {
      stopIntentRef.current = null;
    }
    stopRef.current?.();
    stopRef.current = null;
    setIsStreaming(false);
    setConfirmingToken(null);
    if (streamingConversationId) {
      setConversationDetails((current) => {
        const conversation = current[streamingConversationId];
        if (!conversation) return current;
        return {
          ...current,
          [streamingConversationId]: {
            ...conversation,
            messages: conversation.messages.map((turn) =>
              turn.isStreaming ? { ...turn, isStreaming: false } : turn,
            ),
          },
        };
      });
    }
    setStreamingConversationId(null);
  }, [streamingConversationId]);

  const handleKeyDown = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === 'Enter' && !event.shiftKey) {
      event.preventDefault();
      void sendMessage();
    }
  };

  if (authLoading) {
    return (
      <div className="panel-soft animate-rise px-5 py-4">
        <p className="text-sm muted">Loading…</p>
      </div>
    );
  }

  if (!me) {
    return null;
  }

  const canQueueActiveConversation =
    isStreaming &&
    Boolean(activeConversationId) &&
    activeConversationId === streamingConversationId;
  const composerDisabled =
    serviceUnavailable ||
    inferenceAvailable !== true ||
    !modelStorageAvailable ||
    !selectedModel ||
    streamingElsewhere ||
    Boolean(activeConversationId && !activeConversation) ||
    Boolean(activeConversation?.archived);
  const voiceControlDisabled =
    composerDisabled ||
    isStreaming ||
    voiceState === 'transcribing' ||
    voiceState === 'stopping';
  const queueActionLabel = hasQueuedPrompt ? 'Update queued' : 'Queue';
  const queueActionDisabled = composerDisabled || !input.trim();
  const queuedNotice = hasQueuedPrompt
    ? isStreaming
      ? 'Queued follow-up will send automatically when this answer finishes.'
      : 'Queued follow-up is saved here. Send it when ready, or cancel it.'
    : canQueueActiveConversation
      ? 'You can queue the next prompt while Rustyfin finishes this answer.'
      : null;

  const placeholder = activeConversation?.archived
    ? 'Restore this conversation to keep chatting.'
    : !selectedModel
      ? 'No model available.'
      : canQueueActiveConversation
        ? hasQueuedPrompt
          ? 'Edit the queued follow-up, then press Update queued.'
          : 'Type the next prompt while Rustyfin finishes this answer.'
      : streamingElsewhere
        ? 'Rustyfin AI is still working in another chat.'
        : 'Ask Rustyfin AI something grounded in your server state…';
  const headerSubtitle = activeConversation?.title ??
    liveConversations[0]?.title ??
    'Conversation history';
  const headerStatus = activeConversation?.archived
    ? 'Archived conversation'
    : streamingElsewhere
      ? 'Active response in another chat'
      : null;
  const showRuntimePanel = inferenceAvailable === true && Boolean(selectedModel);
  const showDesktopRuntimePanel = showRuntimePanel && desktopRuntimeOpen;
  const desktopGridClass = desktopRailOpen
    ? showDesktopRuntimePanel
      ? 'md:grid-cols-[15rem_minmax(0,1fr)_16rem] lg:grid-cols-[17rem_minmax(0,1fr)_18rem] xl:grid-cols-[18rem_minmax(0,1fr)_20rem]'
      : 'md:grid-cols-[15rem_minmax(0,1fr)] lg:grid-cols-[17rem_minmax(0,1fr)] xl:grid-cols-[18rem_minmax(0,1fr)]'
    : showDesktopRuntimePanel
      ? 'md:grid-cols-[minmax(0,1fr)_16rem] lg:grid-cols-[minmax(0,1fr)_18rem] xl:grid-cols-[minmax(0,1fr)_20rem]'
      : 'md:grid-cols-[minmax(0,1fr)]';

  return (
    <>
      {drawerOpen ? (
        <div
          className="fixed inset-0 z-40 bg-black/60 md:hidden"
          onClick={() => setDrawerOpen(false)}
        />
      ) : null}

      {runtimeDrawerOpen ? (
        <div
          className="fixed inset-0 z-40 bg-black/60 md:hidden"
          onClick={() => setRuntimeDrawerOpen(false)}
        />
      ) : null}

      <div className="animate-rise relative left-1/2 right-1/2 flex h-full min-h-0 w-screen -translate-x-1/2">
        <div className="flex min-h-0 flex-1 px-[var(--page-pad-inline)]">
          <div className={`grid min-h-0 flex-1 md:overflow-hidden ${desktopGridClass}`}>
            {desktopRailOpen ? (
              <div className="hidden md:flex md:min-h-0 md:flex-col md:overflow-hidden md:border-r md:border-[var(--border)]">
                <div className="flex h-full min-h-0 flex-col">
                  <div className="min-h-0 flex-1">
                    <AiConversationRail
                      conversations={liveConversations}
                      archivedConversations={archivedConversations}
                      activeConversationId={activeConversationId}
                      disabled={conversationsLoading}
                      className="h-full w-full border-r-0 sm:w-full"
                      onSelect={handleSelectConversation}
                      onNewChat={() => {
                        void handleNewChat();
                      }}
                      onRename={handleRenameConversation}
                      onArchiveToggle={(conversation) => {
                        void handleArchiveToggle(conversation);
                      }}
                      onDelete={handleDeleteConversation}
                    />
                  </div>
                </div>
              </div>
            ) : null}

            <div className="flex min-w-0 flex-col md:min-h-0 md:overflow-hidden">
              <div
                className={`fixed inset-y-0 left-0 z-50 flex transition-transform duration-200 md:hidden ${
                  drawerOpen ? 'translate-x-0' : '-translate-x-full'
                }`}
              >
                <div className="flex h-full w-[min(19rem,88vw)] flex-col border-r border-[var(--border)] bg-[rgba(18,22,33,0.98)] shadow-2xl">
                  <div className="flex items-center justify-between border-b border-[var(--border)] px-4 py-3">
                    <div>
                      <p className="text-[0.68rem] font-semibold uppercase tracking-[0.18em] text-[var(--text-faint)]">
                        Conversations
                      </p>
                      <p className="mt-1 text-xs muted">Saved chats and history</p>
                    </div>
                    <button
                      type="button"
                      onClick={() => setDrawerOpen(false)}
                      className="btn-ghost h-9 w-9 rounded-full p-0 text-lg"
                      aria-label="Close conversations"
                    >
                      ×
                    </button>
                  </div>
                  <div className="min-h-0 flex-1">
                    <AiConversationRail
                      conversations={liveConversations}
                      archivedConversations={archivedConversations}
                      activeConversationId={activeConversationId}
                      disabled={conversationsLoading}
                      className="h-full border-r-0 sm:w-full"
                      onSelect={handleSelectConversation}
                      onNewChat={() => {
                        void handleNewChat();
                      }}
                      onRename={handleRenameConversation}
                      onArchiveToggle={(conversation) => {
                        void handleArchiveToggle(conversation);
                      }}
                      onDelete={handleDeleteConversation}
                    />
                  </div>
                </div>
              </div>

              <div
                className={`fixed inset-x-0 bottom-0 z-[60] max-h-[78dvh] overflow-hidden border-t border-[var(--border)] bg-[rgba(18,22,33,0.98)] shadow-2xl transition-transform duration-200 md:hidden ${
                  runtimeDrawerOpen ? 'translate-y-0' : 'pointer-events-none translate-y-full'
                }`}
              >
                <div className="flex items-center justify-between border-b border-[var(--border)] px-4 py-3">
                  <div>
                    <p className="text-[0.68rem] font-semibold uppercase tracking-[0.18em] text-[var(--text-faint)]">
                      AI Runtime
                    </p>
                    <p className="mt-1 text-xs muted">Model, turn, host, and GPU status</p>
                  </div>
                  <button
                    type="button"
                    onClick={() => setRuntimeDrawerOpen(false)}
                    className="btn-ghost h-9 w-9 rounded-full p-0 text-lg"
                    aria-label="Close runtime panel"
                  >
                    ×
                  </button>
                </div>
                <div className="overflow-y-auto p-4">
                  <RuntimePanel
                    runtime={runtime}
                    conversationStats={activeConversationStats}
                    showDetails={showPromptDetails}
                    onToggleDetails={() => setShowPromptDetails((current) => !current)}
                    className="space-y-0"
                    stacked
                  />
                </div>
              </div>

              <section className="flex min-h-0 min-w-0 flex-1 flex-col md:h-full md:overflow-hidden">
                <div className="shrink-0 border-b border-[var(--border)] bg-transparent">
                  <div className="flex flex-col gap-3 px-3 py-3 sm:px-5 sm:py-4 md:flex-row md:items-center md:justify-between">
                    <div className="flex min-w-0 items-center gap-3">
                      <button
                        type="button"
                        className="btn-ghost h-9 w-9 rounded-xl p-0 text-lg leading-none"
                        onClick={() => {
                          if (typeof window !== 'undefined' && window.innerWidth >= 768) {
                            setDesktopRailOpen((current) => !current);
                          } else {
                            setDrawerOpen(true);
                          }
                        }}
                        aria-label={
                          desktopRailOpen
                            ? 'Hide conversations'
                            : 'Show conversations'
                        }
                      >
                        ☰
                      </button>
                      <div className="min-w-0">
                        <h1 className="flex items-center gap-2 text-xl font-semibold tracking-tight">
                          <span className="accent-logo">AI</span>
                          <span className="text-base font-normal text-[var(--text-muted)]">
                            Assistant
                          </span>
                        </h1>
                        <p className="truncate text-xs muted">
                          {headerSubtitle}
                          {headerStatus ? ` · ${headerStatus}` : ''}
                        </p>
                      </div>
                    </div>

                    <div className="flex w-full flex-col gap-2 sm:w-auto sm:flex-row sm:items-center sm:justify-end">
                      {showRuntimePanel ? (
                        <button
                          type="button"
                          onClick={() => {
                            if (typeof window !== 'undefined' && window.innerWidth >= 768) {
                              setDesktopRuntimeOpen((current) => !current);
                            } else {
                              setRuntimeDrawerOpen(true);
                            }
                          }}
                          className="btn-ghost px-4 py-2 text-sm"
                        >
                          {desktopRuntimeOpen ? 'Hide runtime' : 'Runtime'}
                        </button>
                      ) : null}
                      {inferenceAvailable === true && models.length > 0 ? (
                        <ModelSelector
                          models={models}
                          selected={selectedModel}
                          onChange={setSelectedModel}
                        />
                      ) : null}
                      {activeConversation?.archived ? (
                        <button
                          type="button"
                          onClick={() => {
                            const summary = conversations.find(
                              (conversation) => conversation.id === activeConversation.id,
                            );
                            if (summary) {
                              void handleArchiveToggle(summary);
                            }
                          }}
                          className="btn-primary px-4 py-2 text-sm sm:inline-flex"
                        >
                          Restore
                        </button>
                      ) : null}
                    </div>
                  </div>
                </div>

                {modelsError ? (
                  <div className="border-b border-[var(--border)] bg-[rgba(255,145,77,0.08)]">
                    <div className="w-full px-3 py-2 text-sm text-[var(--orange-soft)] sm:px-5">
                      {modelsError}
                    </div>
                  </div>
                ) : null}

                {conversationError ? (
                  <div className="border-b border-[var(--border)] bg-[rgba(255,117,136,0.08)]">
                    <div className="w-full px-3 py-2 text-sm text-[var(--danger)] sm:px-5">
                      {conversationError}
                    </div>
                  </div>
                ) : null}

                {runtimeError ? (
                  <div className="border-b border-[var(--border)] bg-[rgba(255,145,77,0.08)]">
                    <div className="w-full px-3 py-2 text-sm text-[var(--orange-soft)] sm:px-5">
                      {runtimeError}
                    </div>
                  </div>
                ) : null}

                {streamingElsewhere && streamingConversationId ? (
                  <div className="border-b border-[var(--border)] bg-[rgba(255,145,77,0.08)]">
                    <div className="flex w-full flex-wrap items-center gap-2 px-3 py-2 text-sm text-[var(--orange-soft)] sm:px-5">
                      <span>Rustyfin AI is still responding in another chat.</span>
                      <button
                        type="button"
                        onClick={() => handleSelectConversation(streamingConversationId)}
                        className="font-medium text-[var(--text-main)] underline underline-offset-4"
                      >
                        Jump back
                      </button>
                    </div>
                  </div>
                ) : null}

                <div
                  ref={messageScrollRef}
                  onScroll={handleMessageScroll}
                  className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-3 pb-28 pt-5 sm:px-5 sm:pt-6 md:pb-8"
                >
                  <div className="mx-auto w-full max-w-4xl">
                    {serviceUnavailable ? (
                      <div className="flex min-h-[48vh] items-center justify-center">
                        <InferenceUnavailable
                          serviceUnavailable
                          showRecommendation={false}
                        />
                      </div>
                    ) : inferenceAvailable === false ? (
                      <div className="flex min-h-[48vh] items-center justify-center">
                        <InferenceUnavailable
                          title="AI unavailable on this host"
                          description="This host is running without an enabled AI inference backend, so the assistant is unavailable right now."
                          showRecommendation={false}
                        />
                      </div>
                    ) : inferenceAvailable === null ? (
                      <div className="flex min-h-[48vh] items-center justify-center">
                        <span className="muted">Loading AI models…</span>
                      </div>
                    ) : !modelStorageAvailable ? (
                      <div className="flex min-h-[48vh] items-center justify-center">
                        <InferenceUnavailable
                          title="AI model storage is unavailable"
                          description={modelsError ?? 'Rustyfin could not access the configured AI model directory on this host.'}
                          showRecommendation={false}
                        />
                      </div>
                    ) : !selectedModel ? (
                      <div className="flex min-h-[48vh] items-center justify-center">
                        <InferenceUnavailable />
                      </div>
                    ) : activeConversationId &&
                      loadingConversationId === activeConversationId &&
                      !activeConversation ? (
                      <div className="flex min-h-[48vh] items-center justify-center">
                        <span className="muted">Loading conversation…</span>
                      </div>
                    ) : !activeConversation || activeConversation.messages.length === 0 ? (
                      <div className="min-h-[48vh]">
                        <EmptyState
                          model={selectedModel}
                          isAdmin={me.role === 'admin'}
                          hasConversation={Boolean(activeConversation)}
                          suggestionKey={activeConversation?.id ?? activeConversationId ?? 'starter'}
                          onNewChat={() => {
                            void handleNewChat();
                          }}
                          onSuggest={handleSuggestion}
                        />
                      </div>
                    ) : (
                      <div className="flex flex-col gap-4">
                        {activeConversation.messages.map((message) => (
                          <MessageBubble
                            key={message.id}
                            entry={message}
                            showStats={showPromptDetails}
                            onConfirmPendingAction={handleConfirmPendingAction}
                            confirmingToken={confirmingToken}
                            interactionDisabled={isStreaming}
                          />
                        ))}
                      </div>
                    )}
                  </div>
                </div>

                <div className="sticky bottom-0 z-10 shrink-0 border-t border-[rgba(215,223,255,0.08)] bg-transparent">
                  <div className="w-full px-3 pb-[max(env(safe-area-inset-bottom),0px)] pt-3 sm:px-5">
                    <div className="flex min-h-[3.25rem] items-center gap-3">
                      <textarea
                        ref={textareaRef}
                        value={input}
                        onChange={handleInputChange}
                        onKeyDown={handleKeyDown}
                        placeholder={placeholder}
                        className="ai-composer-textarea min-h-[3rem] flex-1 resize-none bg-transparent py-2 text-sm leading-relaxed text-[var(--text-main)] placeholder:text-[var(--text-muted)] disabled:opacity-50"
                        disabled={composerDisabled}
                        rows={1}
                      />

                      <div className="flex shrink-0 -translate-y-0.5 items-center gap-2">
                        {isStreaming ? (
                          <>
                            {canQueueActiveConversation ? (
                              <button
                                type="button"
                                onClick={() => {
                                  void sendMessage();
                                }}
                                className="btn-secondary h-10 px-3.5 text-sm disabled:cursor-not-allowed disabled:opacity-40"
                                disabled={queueActionDisabled}
                              >
                                {queueActionLabel}
                              </button>
                            ) : null}
                            <button
                              type="button"
                              onClick={handleStop}
                              className="btn-primary ai-send-control flex h-10 w-10 items-center justify-center rounded-full p-0"
                              aria-label={hasQueuedPrompt ? 'Stop current response and continue with queued prompt' : 'Stop current response'}
                            >
                              <span className="ai-stop-square" aria-hidden="true" />
                            </button>
                          </>
                        ) : (
                          <>
                            <button
                              type="button"
                              onClick={() => {
                                if (voiceState === 'recording') {
                                  stopVoiceInput();
                                } else {
                                  void startVoiceInput();
                                }
                              }}
                              className="btn-secondary flex h-10 w-10 items-center justify-center rounded-xl p-0 disabled:cursor-not-allowed disabled:opacity-40"
                              disabled={voiceControlDisabled}
                              aria-label={voiceState === 'recording' ? 'Stop voice input' : 'Start voice input'}
                            >
                              <svg
                                className="h-4 w-4"
                                viewBox="0 0 24 24"
                                fill={voiceState === 'recording' ? 'currentColor' : 'none'}
                                stroke="currentColor"
                                strokeWidth="1.9"
                              >
                                <rect x="9" y="3" width="6" height="12" rx="3" />
                                <path d="M6 11a6 6 0 0 0 12 0" />
                                <path d="M12 17v4" />
                              </svg>
                            </button>
                            <button
                              type="button"
                              onClick={() => {
                                void sendMessage();
                              }}
                              className="btn-primary flex h-10 w-10 items-center justify-center rounded-xl p-0 disabled:cursor-not-allowed disabled:opacity-40"
                              disabled={composerDisabled || !input.trim()}
                              aria-label="Send message"
                            >
                              <svg
                                className="h-4 w-4"
                                viewBox="0 0 24 24"
                                fill="none"
                                stroke="currentColor"
                                strokeWidth="1.9"
                              >
                                <path d="M5 12h12" />
                                <path d="m13 6 6 6-6 6" />
                              </svg>
                            </button>
                          </>
                        )}
                      </div>
                    </div>

                    {voiceError || queuedNotice ? (
                      <div className="mt-2 space-y-1.5 px-1 text-xs">
                        {voiceError ? (
                          <div className="text-[var(--danger)]">{voiceError}</div>
                        ) : null}

                        {queuedNotice ? (
                          <div className="flex flex-wrap items-center justify-between gap-3">
                            <span className="muted">{queuedNotice}</span>
                            {hasQueuedPrompt ? (
                              <button
                                type="button"
                                onClick={() => {
                                  if (activeConversationId) {
                                    clearQueuedPrompt(activeConversationId);
                                  }
                                }}
                                className="text-[0.72rem] font-medium text-[var(--text-main)] underline underline-offset-4"
                              >
                                Cancel queued
                              </button>
                            ) : null}
                          </div>
                        ) : null}
                      </div>
                    ) : null}
                  </div>
                </div>
              </section>
            </div>

            {showDesktopRuntimePanel ? (
              <aside className="hidden md:flex md:min-h-0 md:flex-col md:overflow-hidden md:border-l md:border-[var(--border)]">
                <div className="flex h-full min-h-0 flex-col">
                  <div className="min-h-0 flex-1 overflow-y-auto p-4">
                    <RuntimePanel
                      runtime={runtime}
                      conversationStats={activeConversationStats}
                      showDetails={showPromptDetails}
                      onToggleDetails={() => setShowPromptDetails((current) => !current)}
                      className="space-y-0"
                      stacked
                    />
                  </div>
                </div>
              </aside>
            ) : null}
          </div>
        </div>
      </div>

      <ConfirmModal
        open={Boolean(renameTarget)}
        title="Rename conversation"
        description="Choose a clearer title for this stored chat."
        confirmLabel="Save"
        onConfirm={() => {
          void confirmRenameConversation();
        }}
        onCancel={() => {
          if (!modalBusy) {
            setRenameTarget(null);
            setRenameValue('');
          }
        }}
        confirmDisabled={modalBusy || !renameValue.trim()}
        cancelDisabled={modalBusy}
      >
        <input
          value={renameValue}
          onChange={(event) => setRenameValue(event.target.value)}
          className="panel w-full rounded-xl px-3 py-2 text-sm"
          placeholder="Conversation title"
          maxLength={80}
          autoFocus
        />
      </ConfirmModal>

      <ConfirmModal
        open={Boolean(deleteTarget)}
        title="Delete conversation"
        description={
          deleteTarget
            ? `Delete "${deleteTarget.title}" and its saved turns from your AI history? Admin audit history is not affected.`
            : undefined
        }
        confirmLabel="Delete"
        destructive
        onConfirm={() => {
          void confirmDeleteConversation();
        }}
        onCancel={() => {
          if (!modalBusy) {
            setDeleteTarget(null);
          }
        }}
        confirmDisabled={modalBusy}
        cancelDisabled={modalBusy}
      />
    </>
  );
}
