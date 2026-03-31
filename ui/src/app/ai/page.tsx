'use client';

import { useEffect, useRef, useState, useCallback } from 'react';
import { useRouter } from 'next/navigation';
import { useAuth } from '@/lib/auth';
import {
  type AiFollowUpContext,
  fetchModels,
  streamChat,
  type AiModel,
  type AiGroundingSource,
  type AiStatusUpdate,
  type ChatHistoryMessage,
} from '@/lib/aiApi';

// ---------------------------------------------------------------------------
// Chat types
// ---------------------------------------------------------------------------
interface MessageStats {
  prompt_tokens: number;
  completion_tokens: number;
  total_duration_ms: number;
  tokens_per_second: number;
}

interface ChatEntry {
  id: string;
  role: 'user' | 'assistant';
  content: string;
  isStreaming: boolean;
  stats: MessageStats | null;
  error: string | null;
  groundingSources: AiGroundingSource[];
  followUpContexts: AiFollowUpContext[];
  statusUpdates: AiStatusUpdate[];
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------
function uid(): string {
  return Math.random().toString(36).slice(2, 10);
}

function formatMs(ms: number): string {
  return ms < 1000 ? `${ms}ms` : `${(ms / 1000).toFixed(1)}s`;
}

function formatTps(tps: number): string {
  return tps > 0 ? `${tps.toFixed(1)} t/s` : '—';
}

// Detect and strip <think>…</think> from content
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

// ---------------------------------------------------------------------------
// Small reusable pieces
// ---------------------------------------------------------------------------

function StreamingDots() {
  return (
    <span className="inline-flex items-center gap-1 ml-0.5">
      {[0, 1, 2].map((i) => (
        <span
          key={i}
          className="inline-block w-1.5 h-1.5 rounded-full bg-[var(--text-muted)]"
          style={{ animation: `bounce-dot 1.2s ease-in-out ${i * 0.2}s infinite` }}
        />
      ))}
    </span>
  );
}

function ThinkingBlock({ text, live }: { text: string; live: boolean }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="mb-3">
      <button
        onClick={() => setOpen((o) => !o)}
        className="flex items-center gap-2 text-xs muted hover:text-[var(--text-main)] transition-colors"
      >
        <span
          className="w-3 h-3 rounded-full border border-[var(--purple)] flex-shrink-0"
          style={{
            background: live ? 'var(--purple)' : 'transparent',
            animation: live ? 'pulse 1.3s ease-in-out infinite' : 'none',
            opacity: live ? 1 : 0.6,
          }}
        />
        <span className="font-medium">{live ? 'Thinking…' : open ? 'Hide thinking' : 'Show thinking'}</span>
        {!live && (
          <svg className={`w-3 h-3 transition-transform ${open ? 'rotate-180' : ''}`} viewBox="0 0 16 16" fill="currentColor">
            <path d="M8 11L2 5h12z" />
          </svg>
        )}
      </button>
      {(open || live) && (
        <div
          className="mt-2 px-3 py-2.5 text-xs muted leading-relaxed rounded-xl font-mono whitespace-pre-wrap"
          style={{ background: 'rgba(177,140,255,0.07)', border: '1px solid rgba(177,140,255,0.2)' }}
        >
          {text}
          {live && <span className="ai-cursor" />}
        </div>
      )}
    </div>
  );
}

function StatsBar({ stats }: { stats: MessageStats }) {
  return (
    <div className="flex flex-wrap gap-x-4 gap-y-1 mt-2.5 pt-2.5 border-t border-[var(--border)] text-[0.7rem]">
      <span className="flex gap-1"><span className="muted">Prompt</span><span className="font-semibold tabular-nums muted">{stats.prompt_tokens} tok</span></span>
      <span className="flex gap-1"><span className="muted">Output</span><span className="font-semibold tabular-nums muted">{stats.completion_tokens} tok</span></span>
      <span className="flex gap-1"><span className="muted">Speed</span><span className="font-semibold tabular-nums text-[var(--orange-soft)]">{formatTps(stats.tokens_per_second)}</span></span>
      <span className="flex gap-1"><span className="muted">Time</span><span className="font-semibold tabular-nums muted">{formatMs(stats.total_duration_ms)}</span></span>
    </div>
  );
}

function StatusList({ updates }: { updates: AiStatusUpdate[] }) {
  if (updates.length === 0) return null;

  return (
    <div className="mb-2.5 space-y-1.5">
      {updates.map((update) => {
        const isChecking = update.kind === 'checking';
        const isError = update.kind === 'error';
        return (
          <div
            key={`${update.tool}-${update.kind}-${update.label}`}
            className="flex items-center gap-2 text-[0.72rem] muted"
          >
            <span
              className="inline-flex items-center justify-center w-4 h-4 rounded-full border flex-shrink-0"
              style={{
                borderColor: isError
                  ? 'rgba(255,117,136,0.35)'
                  : isChecking
                    ? 'rgba(255,145,77,0.3)'
                    : 'rgba(91,214,136,0.3)',
                background: isError
                  ? 'rgba(255,117,136,0.12)'
                  : isChecking
                    ? 'rgba(255,145,77,0.12)'
                    : 'rgba(91,214,136,0.12)',
              }}
            >
              {isChecking ? (
                <span
                  className="inline-block w-1.5 h-1.5 rounded-full"
                  style={{
                    background: 'var(--orange-soft)',
                    animation: 'pulse 1.2s ease-in-out infinite',
                  }}
                />
              ) : isError ? (
                <span className="text-[var(--danger)] leading-none">!</span>
              ) : (
                <span className="text-[var(--ok)] leading-none">✓</span>
              )}
            </span>
            <span>{update.label}</span>
          </div>
        );
      })}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Message bubble
// ---------------------------------------------------------------------------
function MessageBubble({ entry }: { entry: ChatEntry }) {
  const isUser = entry.role === 'user';
  const { thinking, content } = parseContent(entry.content);

  if (isUser) {
    return (
      <div className="flex justify-end">
        <div
          className="max-w-[78%] px-4 py-2.5 rounded-2xl rounded-br-sm text-sm leading-relaxed whitespace-pre-wrap"
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

  return (
    <div className="flex justify-start">
      <div className="max-w-[84%] w-full">
        {entry.groundingSources.length > 0 && (
          <div className="flex flex-wrap gap-2 mb-2">
            {entry.groundingSources.map((source) => (
              <span
                key={`${entry.id}-${source.tool}-${source.label}`}
                className="chip text-[0.68rem]"
                style={{
                  borderColor:
                    source.status === 'error'
                      ? 'rgba(255,120,120,0.32)'
                      : 'rgba(255,145,77,0.24)',
                  background:
                    source.status === 'error'
                      ? 'rgba(255,120,120,0.08)'
                      : 'rgba(255,145,77,0.08)',
                }}
              >
                {source.label}
              </span>
            ))}
          </div>
        )}

        {entry.isStreaming && entry.statusUpdates.length > 0 && (
          <StatusList updates={entry.statusUpdates} />
        )}

        {thinking !== null && (
          <ThinkingBlock text={thinking} live={entry.isStreaming && content === ''} />
        )}

        <div className="panel-soft px-4 py-3 rounded-2xl rounded-bl-sm text-sm leading-relaxed">
          {entry.error ? (
            <span className="text-[var(--danger)]">{entry.error}</span>
          ) : content ? (
            <span className="whitespace-pre-wrap">{content}</span>
          ) : entry.isStreaming ? (
            <StreamingDots />
          ) : null}

          {entry.isStreaming && content && <span className="ai-cursor" />}
        </div>

        {entry.stats && !entry.isStreaming && <StatsBar stats={entry.stats} />}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// No-model state
// ---------------------------------------------------------------------------
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
  const resolvedTitle = title ?? (serviceUnavailable ? 'AI unavailable on this host' : 'No model installed');
  const resolvedDescription =
    description ??
    (serviceUnavailable
      ? 'This host is running without an enabled AI inference backend, so the assistant is unavailable right now.'
      : 'No AI models are installed right now. Ask an admin to manage models from Admin > AI.');
  const shouldShowRecommendation = showRecommendation ?? !serviceUnavailable;

  return (
    <div className="flex flex-col items-center justify-center flex-1 gap-5 py-16 text-center px-6">
      <div
        className="w-14 h-14 rounded-2xl flex items-center justify-center flex-shrink-0"
        style={{
          background: 'rgba(157,116,255,0.12)',
          border: '1px solid rgba(157,116,255,0.22)',
        }}
      >
        <svg
          className="w-7 h-7 text-[var(--purple)]"
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
        <p className="text-sm muted max-w-xs">{resolvedDescription}</p>
      </div>
      {shouldShowRecommendation && (
        <span className="chip chip-accent text-[0.7rem]">
          Recommended: llama3.2:3b (~2 GB)
        </span>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Empty / suggestions state
// ---------------------------------------------------------------------------
function EmptyState({
  model,
  isAdmin,
  onSuggest,
}: {
  model: string;
  isAdmin: boolean;
  onSuggest: (s: string) => void;
}) {
  const suggestions = [
    'Do I have "Interstellar" in my library?',
    "What was recently added to my library?",
    "Who has a birthday coming up?",
    'What events are coming up this week?',
    "What was the last call about?",
    "What rooms can I join right now?",
    "Any unread activity in general chat?",
    "What is the temperature in Dublin right now?",
    "Are any YouTube rooms active right now?",
    "What network interfaces are active right now?",
    "What Minecraft servers are online?",
    "What downloads are available right now?",
  ];
  if (isAdmin) {
    suggestions.push('How much RAM is the server using right now?');
    suggestions.push("What services are down right now?");
  }

  return (
    <div className="flex flex-col items-center justify-center h-full gap-6 py-8 text-center px-4">
      <div>
        <div
          className="w-12 h-12 rounded-2xl mx-auto mb-4 flex items-center justify-center"
          style={{
            background: 'linear-gradient(135deg, rgba(255,145,77,0.18), rgba(157,116,255,0.18))',
            border: '1px solid rgba(177,140,255,0.22)',
          }}
        >
          <svg className="w-6 h-6" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6">
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
        <p className="text-sm muted mt-1 max-w-xs">
          I can currently search your libraries and check calendar, rooms, downloads, server status, and account context.
          {isAdmin ? ' Admins can also ask for host runtime stats like RAM, CPU, load, and uptime.' : ''}
        </p>
      </div>

      <div className="grid grid-cols-1 sm:grid-cols-2 gap-2 w-full max-w-lg">
        {suggestions.map((s) => (
          <button
            key={s}
            onClick={() => onSuggest(s)}
            className="text-left px-3.5 py-2.5 text-sm rounded-xl transition-all duration-150"
            style={{ background: 'rgba(255,255,255,0.04)', border: '1px solid var(--border)' }}
            onMouseEnter={(e) => (e.currentTarget.style.borderColor = 'var(--border-strong)')}
            onMouseLeave={(e) => (e.currentTarget.style.borderColor = 'var(--border)')}
          >
            {s}
          </button>
        ))}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Model selector dropdown
// ---------------------------------------------------------------------------
function ModelSelector({ models, selected, onChange }: { models: AiModel[]; selected: string; onChange: (n: string) => void }) {
  return (
    <div className="relative">
      <select
        value={selected}
        onChange={(e) => onChange(e.target.value)}
        className="appearance-none cursor-pointer pl-3 pr-8 py-1.5 text-sm rounded-xl border border-[var(--border)] bg-[var(--surface)] text-[var(--text-main)] focus:outline-none focus:border-[var(--purple)] transition-colors"
      >
        {models.map((m) => (
          <option key={m.name} value={m.name}>
            {modelDisplayName(m.name)}
            {m.parameter_size ? ` (${m.parameter_size})` : ''}
          </option>
        ))}
      </select>
      <svg className="pointer-events-none absolute right-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 muted" viewBox="0 0 16 16" fill="currentColor">
        <path d="M8 11L2 5h12z" />
      </svg>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Main page
// ---------------------------------------------------------------------------
export default function AiPage() {
  const router = useRouter();
  const { me, loading: authLoading } = useAuth();

  const [models, setModels] = useState<AiModel[]>([]);
  const [inferenceAvailable, setInferenceAvailable] = useState<boolean | null>(null);
  const [serviceUnavailable, setServiceUnavailable] = useState(false);
  const [modelStorageAvailable, setModelStorageAvailable] = useState(true);
  const [modelsError, setModelsError] = useState<string | null>(null);
  const [selectedModel, setSelectedModel] = useState('');
  const [messages, setMessages] = useState<ChatEntry[]>([]);
  const [input, setInput] = useState('');
  const [isStreaming, setIsStreaming] = useState(false);

  const threadRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const stopRef = useRef<(() => void) | null>(null);

  useEffect(() => {
    if (!authLoading && !me) router.replace('/login');
  }, [authLoading, me, router]);

  const loadModels = useCallback(() => {
    fetchModels()
      .then((res) => {
        setInferenceAvailable(res.inference_available);
        setServiceUnavailable(res.service_unavailable);
        setModelStorageAvailable(res.model_storage_available);
        setModelsError(res.model_storage_error);
        setModels(res.models);
        if (res.models.length > 0 && !selectedModel) {
          setSelectedModel(res.models[0].name);
        } else if (res.models.length === 0) {
          setSelectedModel('');
        }
      })
      .catch(() => {
        setInferenceAvailable(false);
        setServiceUnavailable(false);
        setModelStorageAvailable(false);
        setModelsError('Failed to connect to the Rustyfin backend. Check that the native runtime is online.');
      });
  }, [selectedModel]);

  useEffect(() => {
    if (!me) return;
    loadModels();
  }, [me]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    const el = threadRef.current;
    if (!el) return;
    el.scrollTo({ top: el.scrollHeight, behavior: 'smooth' });
  }, [messages]);

  const handleInputChange = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
    setInput(e.target.value);
    e.target.style.height = 'auto';
    e.target.style.height = `${Math.min(e.target.scrollHeight, 160)}px`;
  };

  const handleNewChat = useCallback(() => {
    if (isStreaming && stopRef.current) stopRef.current();
    setMessages([]);
    setInput('');
    if (textareaRef.current) {
      textareaRef.current.style.height = 'auto';
      textareaRef.current.focus();
    }
  }, [isStreaming]);

  const sendMessage = useCallback(() => {
    const text = input.trim();
    if (!text || !selectedModel || isStreaming) return;

    const userEntry: ChatEntry = {
      id: uid(), role: 'user', content: text,
      isStreaming: false, stats: null, error: null, groundingSources: [], followUpContexts: [], statusUpdates: [],
    };
    const assistantId = uid();
    const assistantEntry: ChatEntry = {
      id: assistantId, role: 'assistant', content: '',
      isStreaming: true, stats: null, error: null, groundingSources: [], followUpContexts: [], statusUpdates: [],
    };

    setMessages((prev) => [...prev, userEntry, assistantEntry]);
    setInput('');
    if (textareaRef.current) textareaRef.current.style.height = 'auto';
    setIsStreaming(true);

    const history: ChatHistoryMessage[] = messages.map((m) => ({
      role: m.role,
      content: m.content,
      grounding_tools:
        m.role === 'assistant' && m.groundingSources.length > 0
          ? m.groundingSources.map((source) => source.tool)
          : undefined,
      follow_up_contexts:
        m.role === 'assistant' && m.followUpContexts.length > 0
          ? m.followUpContexts
          : undefined,
    }));

    stopRef.current = streamChat(
      selectedModel, text, history,
      (event) => {
        if (event.type === 'status') {
          setMessages((prev) =>
            prev.map((m) => {
              if (m.id !== assistantId) return m;
              const nextUpdates = [...m.statusUpdates];
              const existingIndex = nextUpdates.findIndex((update) => update.tool === event.update.tool);
              if (existingIndex >= 0) {
                nextUpdates[existingIndex] = event.update;
              } else {
                nextUpdates.push(event.update);
              }
              return { ...m, statusUpdates: nextUpdates };
            }),
          );
        } else if (event.type === 'token') {
          setMessages((prev) =>
            prev.map((m) => m.id === assistantId ? { ...m, content: m.content + event.text } : m),
          );
        } else if (event.type === 'grounding') {
          setMessages((prev) =>
            prev.map((m) =>
              m.id === assistantId
                ? {
                    ...m,
                    groundingSources: event.sources,
                    followUpContexts: event.followUpContexts,
                  }
                : m,
            ),
          );
        } else if (event.type === 'stats') {
          setMessages((prev) =>
            prev.map((m) => m.id === assistantId ? { ...m, stats: { ...event } } : m),
          );
        } else if (event.type === 'error') {
          setMessages((prev) =>
            prev.map((m) => m.id === assistantId ? { ...m, error: event.message, isStreaming: false } : m),
          );
          setIsStreaming(false);
        } else if (event.type === 'done') {
          setMessages((prev) =>
            prev.map((m) => m.id === assistantId ? { ...m, isStreaming: false } : m),
          );
          setIsStreaming(false);
        }
      },
      () => {
        setMessages((prev) =>
          prev.map((m) => m.id === assistantId ? { ...m, isStreaming: false } : m),
        );
        setIsStreaming(false);
      },
    );
  }, [input, selectedModel, isStreaming, messages]);

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      sendMessage();
    }
  };

  const handleStop = () => {
    if (stopRef.current) stopRef.current();
    setMessages((prev) => prev.map((m) => m.isStreaming ? { ...m, isStreaming: false } : m));
    setIsStreaming(false);
  };

  // ---------------------------------------------------------------------------
  if (authLoading) {
    return (
      <div className="panel-soft animate-rise px-5 py-4">
        <p className="text-sm muted">Loading…</p>
      </div>
    );
  }
  if (!me) return null;

  return (
    <>
      <style>{`
        @keyframes bounce-dot {
          0%,80%,100%{transform:translateY(0);opacity:.4}
          40%{transform:translateY(-5px);opacity:1}
        }
        @keyframes pulse {
          0%,100%{opacity:.5;transform:scale(.85)}
          50%{opacity:1;transform:scale(1.1)}
        }
        .ai-cursor{
          display:inline-block;width:2px;height:1em;
          background:var(--orange-soft);border-radius:1px;
          margin-left:1px;vertical-align:text-bottom;
          animation:ai-blink 900ms step-end infinite;
        }
        @keyframes ai-blink{0%,100%{opacity:1}50%{opacity:0}}
      `}</style>

      <div className="flex flex-col animate-rise" style={{ height: 'calc(100dvh - 8.5rem)' }}>

        {/* ── Header ────────────────────────────────────────────────────── */}
        <div className="flex-shrink-0 flex items-center justify-between gap-3 mb-3">
          <div className="flex items-center gap-3 min-w-0">
            <h1 className="text-xl font-semibold tracking-tight flex-shrink-0">
              <span className="accent-logo">AI</span>
              <span className="text-[var(--text-muted)] font-normal ml-2 text-base">Assistant</span>
            </h1>

            {inferenceAvailable && models.length > 0 && (
              <ModelSelector models={models} selected={selectedModel} onChange={setSelectedModel} />
            )}

            {inferenceAvailable === true && (
              <span className="chip chip-accent text-[0.65rem] flex-shrink-0">
                <span
                  className="inline-block w-1.5 h-1.5 rounded-full"
                  style={{ background: selectedModel ? 'var(--ok)' : 'var(--text-muted)' }}
                />
                {selectedModel ? 'ready' : 'no model'}
              </span>
            )}
          </div>

          <div className="flex items-center gap-2 flex-shrink-0">
            {messages.length > 0 && (
              <button
                onClick={handleNewChat}
                className="px-3 py-1.5 text-xs rounded-xl border border-[var(--border)] muted hover:text-[var(--text-main)] transition-colors"
              >
                New chat
              </button>
            )}
          </div>
        </div>

        {/* ── Body row ───────────────────────────────────────────────────── */}
        <div className="panel flex-1 flex overflow-hidden">

          {/* Chat area */}
          <div className="flex flex-col flex-1 overflow-hidden">
            {serviceUnavailable && (
              <InferenceUnavailable
                serviceUnavailable
                description={
                  modelsError ||
                  'This host is running without an enabled AI inference backend, so the assistant is unavailable right now.'
                }
                showRecommendation={false}
              />
            )}
            {!serviceUnavailable && inferenceAvailable === false && (
              <InferenceUnavailable
                title="AI needs admin attention"
                description={
                  modelsError ||
                  'Rustyfin could not read local AI models from the configured storage folder. Ask an admin to review Admin > AI.'
                }
                showRecommendation={false}
              />
            )}
            {inferenceAvailable === null && (
              <div className="flex-1 flex items-center justify-center">
                <p className="text-sm muted">Loading…</p>
              </div>
            )}
            {inferenceAvailable === true && !modelStorageAvailable && (
              <InferenceUnavailable
                title="AI model storage is unavailable"
                description={
                  modelsError ||
                  'Rustyfin cannot read the configured AI model folder. Ask an admin to review Admin > AI.'
                }
                showRecommendation={false}
              />
            )}
            {inferenceAvailable === true && !selectedModel && (
              <InferenceUnavailable />
            )}

            {inferenceAvailable === true && modelStorageAvailable && selectedModel && (
              <>
                <div ref={threadRef} className="flex-1 overflow-y-auto px-5 py-5 space-y-5">
                  {messages.length === 0 && (
                    <EmptyState
                      model={selectedModel}
                      isAdmin={me?.role === 'admin'}
                      onSuggest={(s) => setInput(s)}
                    />
                  )}
                  {messages.map((entry) => (
                    <MessageBubble key={entry.id} entry={entry} />
                  ))}
                </div>

                {/* Input bar */}
                <div className="flex-shrink-0 px-4 pb-4 pt-2 border-t border-[var(--border)]">
                  <div
                    className="flex items-end gap-2 rounded-2xl px-3 py-2"
                    style={{
                      background: 'rgba(255,255,255,0.04)',
                      border: '1px solid var(--border-strong)',
                    }}
                  >
                    <textarea
                      ref={textareaRef}
                      value={input}
                      onChange={handleInputChange}
                      onKeyDown={handleKeyDown}
                      placeholder="Message… (Ctrl+Enter to send)"
                      rows={1}
                      disabled={isStreaming}
                      className="flex-1 bg-transparent text-sm text-[var(--text-main)] placeholder:text-[var(--text-muted)] resize-none focus:outline-none py-1 leading-relaxed disabled:opacity-50"
                      style={{ maxHeight: '160px', minHeight: '1.5rem' }}
                    />
                    {isStreaming ? (
                      <button
                        onClick={handleStop}
                        className="flex-shrink-0 w-8 h-8 flex items-center justify-center rounded-xl transition-colors"
                        style={{ background: 'rgba(255,117,136,0.16)', border: '1px solid rgba(255,117,136,0.32)' }}
                        title="Stop generation"
                      >
                        <svg className="w-3.5 h-3.5 text-[var(--danger)]" viewBox="0 0 16 16" fill="currentColor">
                          <rect x="3" y="3" width="10" height="10" rx="1.5" />
                        </svg>
                      </button>
                    ) : (
                      <button
                        onClick={sendMessage}
                        disabled={!input.trim() || !selectedModel}
                        className="btn-primary flex-shrink-0 w-8 h-8 flex items-center justify-center rounded-xl disabled:opacity-40 disabled:cursor-not-allowed"
                        title="Send (Ctrl+Enter)"
                      >
                        <svg className="w-3.5 h-3.5" viewBox="0 0 16 16" fill="currentColor">
                          <path d="M2 13.5L14 8 2 2.5V6.5l8 1.5-8 1.5z" />
                        </svg>
                      </button>
                    )}
                  </div>
                  <p className="text-[0.65rem] muted mt-1.5 ml-1">
                    {selectedModel ? `${modelDisplayName(selectedModel)} · Ctrl+Enter to send` : 'Select a model above'}
                  </p>
                </div>
              </>
            )}
          </div>
        </div>
      </div>
    </>
  );
}
