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
  type AiGroundingChunk,
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

type VoiceState = 'idle' | 'recording' | 'stopping' | 'transcribing' | 'ready' | 'error';

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
    <div className="mb-3">
      <button
        type="button"
        onClick={() => setOpen((current) => !current)}
        className="flex items-center gap-2 text-xs muted transition-colors hover:text-[var(--text-main)]"
      >
        <span
          className="h-3 w-3 rounded-full border border-[var(--purple)]"
          style={{
            background: live ? 'var(--purple)' : 'transparent',
            opacity: live ? 1 : 0.6,
          }}
        />
        <span className="font-medium">
          {live ? 'Thinking...' : open ? 'Hide fallback note' : 'Show fallback note'}
        </span>
        {!live ? (
          <svg
            className={`h-3 w-3 transition-transform ${open ? 'rotate-180' : ''}`}
            viewBox="0 0 16 16"
            fill="currentColor"
          >
            <path d="M8 11L2 5h12z" />
          </svg>
        ) : null}
      </button>
      {open ? (
        <div
          className="mt-2 rounded-xl px-3 py-2.5 font-mono text-xs leading-relaxed muted whitespace-pre-wrap"
          style={{
            background: 'rgba(177,140,255,0.07)',
            border: '1px solid rgba(177,140,255,0.2)',
          }}
        >
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
  className = '',
  stacked = false,
}: {
  runtime: AiRuntimeResponse | null;
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
      <div className={stacked ? 'grid gap-2' : 'grid gap-2 md:grid-cols-2 xl:grid-cols-4'}>
        <div className="panel-soft rounded-2xl px-3 py-3">
          <p className="text-[0.68rem] uppercase tracking-[0.14em] muted">Model</p>
          <p className="mt-1 text-sm font-semibold">
            {runtime.model.name ?? 'No model loaded'}
          </p>
          <p className="mt-1 text-xs muted">
            {runtime.model.backend} · ctx {runtime.model.context_length} · {runtime.model.n_threads} threads
          </p>
          <p className="mt-1 text-xs muted">
            {runtimeSplitModeLabel(runtime.model.split_mode)} · {gpuSummary}
          </p>
          {configuredDevices ? (
            <p className="mt-1 text-xs muted">{configuredDevices}</p>
          ) : null}
        </div>
        <div className="panel-soft rounded-2xl px-3 py-3">
          <p className="text-[0.68rem] uppercase tracking-[0.14em] muted">Turn</p>
          <p className="mt-1 text-sm font-semibold">
            {runtimePhaseLabel(runtime.turn.phase)}
          </p>
          <p className="mt-1 text-xs muted">
            {runtime.turn.active_request_count} active · queue {runtime.turn.queue_depth}
          </p>
        </div>
        <div className="panel-soft rounded-2xl px-3 py-3">
          <p className="text-[0.68rem] uppercase tracking-[0.14em] muted">Resources</p>
          <p className="mt-1 text-sm font-semibold">
            {runtime.resources.process_rss_human ?? '—'}
          </p>
          <p className="mt-1 text-xs muted">
            CPU {formatPercent(runtime.resources.host_cpu_percent, 1)}
          </p>
          <p className="mt-1 text-xs muted">
            RAM {runtimeRamSummary(runtime.resources)}
          </p>
        </div>
        <div className="panel-soft rounded-2xl px-3 py-3">
          <p className="text-[0.68rem] uppercase tracking-[0.14em] muted">GPUs</p>
          {runtime.gpus.length > 0 ? (
            <div className="mt-2 space-y-2">
              {runtime.gpus.map((gpu) => (
                <div
                  key={`${gpu.index ?? 'gpu'}-${gpu.name}`}
                  className="rounded-xl border border-[var(--border)] bg-[rgba(255,255,255,0.03)] px-3 py-2"
                >
                  <p className="text-sm font-semibold">
                    {gpu.index !== undefined && gpu.index !== null ? `GPU ${gpu.index}` : 'GPU'} · {gpu.name}
                  </p>
                  <p className="mt-1 text-xs muted">
                    {formatPercent(gpu.utilization_percent, 0)} · {gpu.vram_used_human ?? '—'} / {gpu.vram_total_human ?? '—'}
                  </p>
                  <p className="mt-1 text-xs muted">
                    Temp {gpu.temperature_celsius ?? '—'}°C
                  </p>
                </div>
              ))}
            </div>
          ) : (
            <p className="mt-1 text-xs muted">No GPU telemetry available for this host runtime.</p>
          )}
        </div>
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

function formatCitationWindow(chunk: AiGroundingChunk): string | null {
  const citation = chunk.citation;
  if (!citation) return null;
  const start = citation.started_ts_ms;
  const end = citation.ended_ts_ms;
  if (start == null && end == null) return null;
  const formatMs = (value: number) => {
    const totalSeconds = Math.max(0, Math.floor(value / 1000));
    const hours = Math.floor(totalSeconds / 3600);
    const minutes = Math.floor((totalSeconds % 3600) / 60);
    const seconds = totalSeconds % 60;
    if (hours > 0) {
      return `${hours.toString().padStart(2, '0')}:${minutes
        .toString()
        .padStart(2, '0')}:${seconds.toString().padStart(2, '0')}`;
    }
    return `${minutes.toString().padStart(2, '0')}:${seconds
      .toString()
      .padStart(2, '0')}`;
  };
  if (start != null && end != null) {
    return `${formatMs(start)}-${formatMs(end)}`;
  }
  return formatMs(start ?? end ?? 0);
}

function SourceList({ chunks }: { chunks: AiGroundingChunk[] }) {
  if (chunks.length === 0) return null;
  return (
    <div className="mt-3 space-y-2">
      <p className="text-[0.68rem] uppercase tracking-[0.14em] muted">Sources</p>
      <div className="space-y-2">
        {chunks.map((chunk) => {
          const citationWindow = formatCitationWindow(chunk);
          return (
            <div
              key={chunk.id}
              className="rounded-xl border border-[var(--border)] bg-[rgba(255,255,255,0.025)] px-3 py-2"
            >
              <div className="flex items-start justify-between gap-3">
                <p className="text-xs font-semibold text-[var(--fg)]">{chunk.title}</p>
                <p className="text-[0.7rem] muted">{chunk.source_kind}</p>
              </div>
              <p className="mt-1 text-xs leading-relaxed text-[var(--fg-soft)] whitespace-pre-wrap">
                {chunk.excerpt}
              </p>
              {chunk.citation ? (
                <p className="mt-2 text-[0.72rem] muted">
                  {chunk.citation.label ? `${chunk.citation.label} · ` : ''}
                  {citationWindow ? `${citationWindow} · ` : ''}
                  {chunk.citation.source_sub_id ?? chunk.citation.source_id}
                </p>
              ) : null}
            </div>
          );
        })}
      </div>
    </div>
  );
}

function MessageBubble({
  entry,
  onConfirmPendingAction,
  confirmingToken,
  interactionDisabled,
}: {
  entry: UiConversationTurn;
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

        <SourceList chunks={entry.grounding_chunks} />

        {entry.stats ? <StatsBar stats={entry.stats} /> : null}

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
  onNewChat,
  onSuggest,
}: {
  model: string;
  isAdmin: boolean;
  hasConversation: boolean;
  onNewChat: () => void;
  onSuggest: (value: string) => void;
}) {
  const suggestions = [
    'How much RAM is the server using right now?',
    'What is that in gigabytes?',
    "What's my next event?",
    'What events are coming up this week?',
    'Who has a birthday coming up?',
    'What was the last call about?',
    'What rooms can I join right now?',
    'Any unread activity in general chat?',
    'What downloads are available right now?',
  ];

  if (isAdmin) {
    suggestions.push('What services are down right now?');
  }

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
        className="w-full appearance-none cursor-pointer rounded-xl border border-[var(--border)] bg-[var(--surface)] py-1.5 pl-3 pr-8 text-sm text-[var(--text-main)] transition-colors focus:border-[var(--purple)] focus:outline-none"
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
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [runtimeDrawerOpen, setRuntimeDrawerOpen] = useState(false);
  const [conversationError, setConversationError] = useState('');
  const [conversationsLoading, setConversationsLoading] = useState(false);
  const [loadingConversationId, setLoadingConversationId] = useState<string | null>(null);

  const [input, setInput] = useState('');
  const [isStreaming, setIsStreaming] = useState(false);
  const [streamingConversationId, setStreamingConversationId] = useState<string | null>(null);
  const [confirmingToken, setConfirmingToken] = useState<string | null>(null);

  const [runtime, setRuntime] = useState<AiRuntimeResponse | null>(null);
  const [runtimeError, setRuntimeError] = useState<string | null>(null);

  const [voiceState, setVoiceState] = useState<VoiceState>('idle');
  const [voiceError, setVoiceError] = useState<string | null>(null);
  const [voiceNotice, setVoiceNotice] = useState<string | null>(null);

  const [renameTarget, setRenameTarget] = useState<AiConversationSummary | null>(null);
  const [renameValue, setRenameValue] = useState('');
  const [deleteTarget, setDeleteTarget] = useState<AiConversationSummary | null>(null);
  const [modalBusy, setModalBusy] = useState(false);

  const threadRef = useRef<HTMLDivElement | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  const stopRef = useRef<(() => void) | null>(null);
  const recognitionRef = useRef<BrowserSpeechRecognition | null>(null);
  const mediaRecorderRef = useRef<MediaRecorder | null>(null);
  const mediaStreamRef = useRef<MediaStream | null>(null);
  const mediaChunksRef = useRef<Blob[]>([]);

  const activeConversation = activeConversationId
    ? conversationDetails[activeConversationId] ?? null
    : null;
  const lastActiveMessageContent =
    activeConversation && activeConversation.messages.length > 0
      ? activeConversation.messages[activeConversation.messages.length - 1].content
      : '';
  const archivedConversations = conversations.filter((conversation) => conversation.archived);
  const liveConversations = conversations.filter((conversation) => !conversation.archived);
  const streamingElsewhere =
    Boolean(streamingConversationId) && streamingConversationId !== activeConversationId;

  const resetComposerHeight = useCallback(() => {
    if (textareaRef.current) {
      textareaRef.current.style.height = 'auto';
    }
  }, []);

  const focusComposer = useCallback(() => {
    textareaRef.current?.focus();
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
    const node = threadRef.current;
    if (!node) return;
    node.scrollTo({ top: node.scrollHeight, behavior: 'smooth' });
  }, [
    activeConversationId,
    activeConversation?.messages.length,
    lastActiveMessageContent,
  ]);

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
      setVoiceState('ready');
      setVoiceError(null);
      setVoiceNotice('Voice transcript ready. Review it, edit if needed, then send.');
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
    setInput(event.target.value);
    if (voiceState === 'ready') {
      setVoiceNotice('Voice transcript ready. Review it, edit if needed, then send.');
    }
    event.target.style.height = 'auto';
    event.target.style.height = `${Math.min(event.target.scrollHeight, 160)}px`;
  };

  const handleSuggestion = useCallback(
    (value: string) => {
      setInput(value);
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

  const handleSelectConversation = useCallback(
    (conversationId: string) => {
      setConversationError('');
      setActiveConversationId(conversationId);
      setDrawerOpen(false);
      setInput('');
      resetComposerHeight();
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
      setActiveConversationId(nextActiveId);
      setDeleteTarget(null);
    } catch (error) {
      setConversationError(
        clientErrorMessage(error, 'Failed to delete this conversation.'),
      );
    } finally {
      setModalBusy(false);
    }
  }, [activeConversationId, conversations, deleteTarget]);

  const startVoiceInput = useCallback(async () => {
    if (voiceState === 'recording' || voiceState === 'stopping' || voiceState === 'transcribing') {
      return;
    }

    setVoiceError(null);
    setVoiceNotice(null);

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
  }) => {
    const confirmationToken = options?.confirmationToken;
    const text = (options?.text ?? input).trim();
    if (!text || !selectedModel || isStreaming) return;
    if (inferenceAvailable !== true || !modelStorageAvailable) return;
    if (activeConversation?.archived) {
      setConversationError('Restore this archived conversation before sending a new message.');
      return;
    }

    let conversationId = activeConversationId;
    let conversationTitle = activeConversation?.title ?? DEFAULT_CONVERSATION_TITLE;

    try {
      setConversationError('');
      if (!conversationId) {
        const detail = await createConversationRecord();
        conversationId = detail.id;
        conversationTitle = detail.title;
      }

      if (!conversationId) {
        throw new Error('No conversation is available for this message.');
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
        grounding_chunks: [],
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
        grounding_chunks: [],
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
      setVoiceNotice(null);

      let completed = false;
      let latestAssistantContent = '';

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
              grounding_chunks: event.chunks,
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

          if (completed) {
            void loadConversationDetail(conversationId!);
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
    resetComposerHeight,
    selectedModel,
    updateAssistantTurn,
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

  const handleStop = useCallback(() => {
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
    if (event.key === 'Enter' && (event.ctrlKey || event.metaKey)) {
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

  const composerDisabled =
    serviceUnavailable ||
    inferenceAvailable !== true ||
    !modelStorageAvailable ||
    !selectedModel ||
    isStreaming ||
    Boolean(activeConversationId && !activeConversation) ||
    Boolean(activeConversation?.archived);
  const voiceControlDisabled =
    composerDisabled || voiceState === 'transcribing' || voiceState === 'stopping';

  const placeholder = activeConversation?.archived
    ? 'Restore this conversation to keep chatting.'
    : !selectedModel
      ? 'No model available.'
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

  return (
    <>
      <div className="flex h-[calc(100dvh-8rem)] overflow-hidden rounded-2xl border border-[var(--border)] animate-rise">
        <div className="hidden h-full shrink-0 sm:flex">
          <AiConversationRail
            conversations={liveConversations}
            archivedConversations={archivedConversations}
            activeConversationId={activeConversationId}
            disabled={conversationsLoading}
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

        {drawerOpen ? (
          <div
            className="fixed inset-0 z-40 bg-black/60 sm:hidden"
            onClick={() => setDrawerOpen(false)}
          />
        ) : null}

        {runtimeDrawerOpen ? (
          <div
            className="fixed inset-0 z-40 bg-black/60 lg:hidden"
            onClick={() => setRuntimeDrawerOpen(false)}
          />
        ) : null}

        <div
          className={`fixed inset-y-0 left-0 z-50 flex transition-transform duration-200 sm:hidden ${
            drawerOpen ? 'translate-x-0' : '-translate-x-full'
          }`}
        >
          <div className="flex h-full w-[min(19rem,88vw)] flex-col">
            <div className="flex items-center justify-between border-b border-[var(--border)] bg-[rgba(10,14,24,0.96)] px-4 py-3">
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
          className={`fixed inset-x-0 bottom-0 z-[60] max-h-[78dvh] overflow-hidden rounded-t-[1.75rem] border border-[var(--border)] bg-[rgba(10,14,24,0.97)] shadow-2xl transition-transform duration-200 lg:hidden ${
            runtimeDrawerOpen ? 'translate-y-0' : 'translate-y-full pointer-events-none'
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
            <RuntimePanel runtime={runtime} className="space-y-0" stacked />
          </div>
        </div>

        <div className="flex min-w-0 flex-1 flex-col overflow-hidden bg-[var(--bg)]">
          <div className="flex shrink-0 flex-col gap-3 border-b border-[var(--border)] px-3 py-3 sm:flex-row sm:items-center sm:justify-between sm:px-5">
            <div className="flex min-w-0 items-center gap-3">
              <button
                type="button"
                className="btn-ghost h-9 w-9 rounded-xl p-0 text-lg leading-none sm:hidden"
                onClick={() => setDrawerOpen(true)}
                aria-label="Open conversations"
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

            <div className="flex w-full flex-col gap-2 sm:w-auto sm:flex-row sm:items-center">
              {showRuntimePanel ? (
                <button
                  type="button"
                  onClick={() => setRuntimeDrawerOpen(true)}
                  className="btn-ghost px-4 py-2 text-sm lg:hidden"
                >
                  Runtime
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

          {modelsError ? (
            <div className="shrink-0 border-b border-[var(--border)] bg-[rgba(255,145,77,0.08)] px-4 py-2 text-sm text-[var(--orange-soft)]">
              {modelsError}
            </div>
          ) : null}

          {conversationError ? (
            <div className="shrink-0 border-b border-[var(--border)] bg-[rgba(255,117,136,0.08)] px-4 py-2 text-sm text-[var(--danger)]">
              {conversationError}
            </div>
          ) : null}

          {runtimeError ? (
            <div className="shrink-0 border-b border-[var(--border)] bg-[rgba(255,145,77,0.08)] px-4 py-2 text-sm text-[var(--orange-soft)]">
              {runtimeError}
            </div>
          ) : null}

          {streamingElsewhere && streamingConversationId ? (
            <div className="shrink-0 border-b border-[var(--border)] bg-[rgba(255,145,77,0.08)] px-4 py-2 text-sm text-[var(--orange-soft)]">
              Rustyfin AI is still responding in another chat.
              <button
                type="button"
                onClick={() => handleSelectConversation(streamingConversationId)}
                className="ml-3 font-medium text-[var(--text-main)] underline underline-offset-4"
              >
                Jump back
              </button>
            </div>
          ) : null}

          <div className="min-h-0 flex flex-1 overflow-hidden">
            <div className="min-h-0 flex min-w-0 flex-1 flex-col overflow-hidden">
              <div className="min-h-0 flex-1 overflow-hidden">
                {serviceUnavailable ? (
                  <InferenceUnavailable
                    serviceUnavailable
                    showRecommendation={false}
                  />
                ) : inferenceAvailable === false ? (
                  <InferenceUnavailable
                    title="AI unavailable on this host"
                    description="This host is running without an enabled AI inference backend, so the assistant is unavailable right now."
                    showRecommendation={false}
                  />
                ) : inferenceAvailable === null ? (
                  <div className="flex h-full items-center justify-center">
                    <span className="muted">Loading AI models…</span>
                  </div>
                ) : !modelStorageAvailable ? (
                  <InferenceUnavailable
                    title="AI model storage is unavailable"
                    description={modelsError ?? 'Rustyfin could not access the configured AI model directory on this host.'}
                    showRecommendation={false}
                  />
                ) : !selectedModel ? (
                  <InferenceUnavailable />
                ) : activeConversationId && loadingConversationId === activeConversationId && !activeConversation ? (
                  <div className="flex h-full items-center justify-center">
                    <span className="muted">Loading conversation…</span>
                  </div>
                ) : !activeConversation || activeConversation.messages.length === 0 ? (
                  <EmptyState
                    model={selectedModel}
                    isAdmin={me.role === 'admin'}
                    hasConversation={Boolean(activeConversation)}
                    onNewChat={() => {
                      void handleNewChat();
                    }}
                    onSuggest={handleSuggestion}
                  />
                ) : (
                  <div
                    ref={threadRef}
                    className="h-full overflow-y-auto px-3 py-4 sm:px-5 sm:py-5"
                  >
                    <div className="mx-auto flex w-full max-w-4xl flex-col gap-4">
                      {activeConversation.messages.map((message) => (
                        <MessageBubble
                          key={message.id}
                          entry={message}
                          onConfirmPendingAction={handleConfirmPendingAction}
                          confirmingToken={confirmingToken}
                          interactionDisabled={isStreaming}
                        />
                      ))}
                    </div>
                  </div>
                )}
              </div>

              <div className="shrink-0 border-t border-[var(--border)] bg-[rgba(13,17,28,0.92)] px-3 py-3 sm:px-5">
                <div className="mx-auto w-full max-w-4xl">
                  <div
                    className="panel-soft flex items-end gap-3 rounded-2xl border border-[var(--border)] px-3 py-3"
                    style={{ background: 'rgba(19,24,38,0.9)' }}
                  >
                    <textarea
                      ref={textareaRef}
                      value={input}
                      onChange={handleInputChange}
                      onKeyDown={handleKeyDown}
                      placeholder={placeholder}
                      className="min-h-[2.4rem] flex-1 resize-none bg-transparent py-1 text-sm leading-relaxed text-[var(--text-main)] placeholder:text-[var(--text-muted)] focus:outline-none disabled:opacity-50"
                      disabled={composerDisabled}
                      rows={1}
                    />

                    {isStreaming ? (
                      <button
                        type="button"
                        onClick={handleStop}
                        className="btn-secondary h-10 px-4 text-sm"
                      >
                        Stop
                      </button>
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
                          <svg className="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.9">
                            <path d="M5 12h12" />
                            <path d="m13 6 6 6-6 6" />
                          </svg>
                        </button>
                      </>
                    )}
                  </div>

                  {voiceError || voiceNotice || voiceState !== 'idle' ? (
                    <div className="mt-2 px-1 text-xs">
                      {voiceError ? (
                        <span className="text-[var(--danger)]">{voiceError}</span>
                      ) : voiceNotice ? (
                        <span className="text-[var(--orange-soft)]">{voiceNotice}</span>
                      ) : (
                        <span className="muted">
                          {voiceState === 'recording'
                            ? 'Listening…'
                            : voiceState === 'stopping'
                              ? 'Stopping…'
                              : voiceState === 'transcribing'
                                ? 'Transcribing…'
                                : null}
                        </span>
                      )}
                    </div>
                  ) : null}
                </div>
              </div>
            </div>

            {showRuntimePanel ? (
              <aside className="hidden lg:flex lg:w-[22rem] xl:w-[24rem] shrink-0 border-l border-[var(--border)] bg-[rgba(8,12,20,0.6)]">
                <div className="min-h-0 flex-1 overflow-y-auto">
                  <div className="border-b border-[var(--border)] px-4 py-3">
                    <p className="text-[0.68rem] font-semibold uppercase tracking-[0.18em] text-[var(--text-faint)]">
                      AI Runtime
                    </p>
                    <p className="mt-1 text-xs muted">Model, turn, host, and GPU status</p>
                  </div>
                  <RuntimePanel runtime={runtime} className="p-4" stacked />
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
