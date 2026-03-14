'use client';

import { useEffect, useRef, useState, useCallback } from 'react';
import { useRouter } from 'next/navigation';
import { useAuth } from '@/lib/auth';
import {
  fetchModels,
  fetchRunningModels,
  fetchGpus,
  deleteModel,
  pullModel,
  streamChat,
  type AiModel,
  type RunningModel,
  type GpuInfo,
  type GpusResponse,
  type ChatHistoryMessage,
  type PullSseEvent,
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
}

// Pull state per-model-name
interface PullState {
  status: string;
  percent: number;
  active: boolean;
  done: boolean;
  error: string | null;
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

function formatGb(gb: number): string {
  return gb > 0 ? `${gb.toFixed(1)} GB` : '—';
}

function formatMb(mb: number): string {
  if (mb >= 1024) return `${(mb / 1024).toFixed(1)} GB`;
  return `${mb} MB`;
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

function VramBar({ used, total }: { used: number; total: number }) {
  const pct = total > 0 ? Math.min(100, (used / total) * 100) : 0;
  const danger = pct > 85;
  const warn = pct > 65;
  return (
    <div className="flex items-center gap-2 text-[0.7rem]">
      <div className="flex-1 rf-progress-track" style={{ height: '6px' }}>
        <div
          className="rf-progress-fill h-full transition-all duration-500"
          style={{
            width: `${pct}%`,
            background: danger
              ? 'var(--danger)'
              : warn
              ? 'linear-gradient(90deg, var(--orange), var(--danger))'
              : undefined,
          }}
        />
      </div>
      <span className="muted tabular-nums whitespace-nowrap">
        {formatMb(used)} / {formatMb(total)}
      </span>
    </div>
  );
}

// ---------------------------------------------------------------------------
// GPU Info panel section
// ---------------------------------------------------------------------------
function GpuSection({ gpus, gpuNote, cudaEnv, running }: {
  gpus: GpuInfo[];
  gpuNote: string;
  cudaEnv: string | null;
  running: RunningModel[];
}) {
  if (gpus.length === 0) {
    return (
      <div className="space-y-2">
        <p className="text-xs font-semibold text-[var(--text-muted)] uppercase tracking-wider">GPU</p>
        <p className="text-xs muted">No NVIDIA GPUs detected via nvidia-smi.</p>
        <p className="text-xs muted">AMD ROCm GPUs are managed automatically by llama.cpp when present.</p>
      </div>
    );
  }

  return (
    <div className="space-y-3">
      <p className="text-xs font-semibold text-[var(--text-muted)] uppercase tracking-wider">
        GPU{gpus.length > 1 ? `s — ${gpus.length} detected` : ''}
      </p>

      {gpus.map((gpu) => (
        <div key={gpu.index} className="space-y-1.5">
          <div className="flex items-center justify-between gap-2">
            <span className="text-xs font-medium truncate">
              <span className="muted mr-1.5">#{gpu.index}</span>
              {gpu.name}
            </span>
            {gpu.utilization_pct != null && (
              <span className="chip text-[0.65rem] flex-shrink-0">{gpu.utilization_pct}% util</span>
            )}
          </div>
          <VramBar used={gpu.vram_used_mb} total={gpu.vram_total_mb} />
        </div>
      ))}

      {/* Multi-GPU note */}
      {gpus.length > 1 && (
        <div className="px-3 py-2 rounded-xl text-[0.68rem] muted leading-relaxed"
          style={{ background: 'rgba(177,140,255,0.07)', border: '1px solid rgba(177,140,255,0.18)' }}>
          <span className="text-[var(--purple)] font-semibold">Multi-GPU: </span>
          {gpuNote}
        </div>
      )}

      {/* CUDA_VISIBLE_DEVICES */}
      <div className="text-[0.68rem] muted space-y-0.5">
        <p>
          <span className="font-semibold">CUDA_VISIBLE_DEVICES: </span>
          {cudaEnv ? (
            <span className="font-mono text-[var(--text-main)]">{cudaEnv}</span>
          ) : (
            <span className="italic">not set — all {gpus.length} GPU{gpus.length > 1 ? 's' : ''} available to rustfin-server</span>
          )}
        </p>
        <p>Set this env var before starting <span className="font-mono">rustfin-server</span> to restrict GPU access.</p>
      </div>

      {/* Currently loaded models */}
      {running.length > 0 && (
        <div className="space-y-1.5 pt-1 border-t border-[var(--border)]">
          <p className="text-[0.68rem] font-semibold muted uppercase tracking-wider">Loaded in VRAM</p>
          {running.map((m) => (
            <div key={m.name} className="flex items-center justify-between gap-2 text-xs">
              <span className="font-mono truncate">{m.name}</span>
              <span className="chip flex-shrink-0">{formatGb(m.size_vram_gb)}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Pull model section
// ---------------------------------------------------------------------------
function PullSection({
  onPulled,
}: {
  onPulled: () => void;
}) {
  const [input, setInput] = useState('');
  const [state, setState] = useState<PullState | null>(null);
  const stopRef = useRef<(() => void) | null>(null);

  const startPull = useCallback(() => {
    const name = input.trim();
    if (!name || state?.active) return;

    setState({ status: 'Starting…', percent: 0, active: true, done: false, error: null });

    stopRef.current = pullModel(
      name,
      (event: PullSseEvent) => {
        if (event.type === 'progress') {
          setState((prev) => ({
            ...(prev ?? { active: true, done: false, error: null }),
            status: event.status,
            percent: event.percent,
            active: true,
            done: false,
            error: null,
          }));
        } else if (event.type === 'done') {
          setState({ status: 'Complete', percent: 100, active: false, done: true, error: null });
          setInput('');
          onPulled();
        } else if (event.type === 'error') {
          setState((prev) => ({
            ...(prev ?? { percent: 0, done: false }),
            status: 'Failed',
            active: false,
            done: false,
            error: event.message,
          }));
        }
      },
      () => {
        setState((prev) =>
          prev?.active ? { ...prev, active: false, status: 'Cancelled' } : prev,
        );
      },
    );
  }, [input, state, onPulled]);

  const cancel = () => {
    if (stopRef.current) stopRef.current();
  };

  const popular = ['llama3.2:3b', 'qwen2.5:7b', 'mistral:7b', 'deepseek-r1:8b'];

  return (
    <div className="space-y-3">
      <p className="text-xs font-semibold text-[var(--text-muted)] uppercase tracking-wider">Install Model</p>

      {/* Input row */}
      <div className="flex gap-2">
        <input
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && startPull()}
          placeholder="model:tag  e.g. llama3.2:3b"
          disabled={state?.active}
          className="flex-1 bg-transparent text-sm px-3 py-1.5 rounded-xl border border-[var(--border)] text-[var(--text-main)] placeholder:muted focus:outline-none focus:border-[var(--purple)] disabled:opacity-50 transition-colors"
        />
        {state?.active ? (
          <button onClick={cancel} className="btn-danger px-3 py-1.5 text-xs rounded-xl">Stop</button>
        ) : (
          <button
            onClick={startPull}
            disabled={!input.trim()}
            className="btn-primary px-3 py-1.5 text-xs rounded-xl disabled:opacity-40"
          >
            Pull
          </button>
        )}
      </div>

      {/* Quick picks */}
      <div className="flex flex-wrap gap-1.5">
        {popular.map((name) => (
          <button
            key={name}
            onClick={() => setInput(name)}
            disabled={state?.active}
            className="chip text-[0.65rem] hover:border-[var(--border-strong)] transition-colors disabled:opacity-40"
          >
            {name}
          </button>
        ))}
      </div>

      {/* Progress */}
      {state && (
        <div className="space-y-1.5">
          <div className="flex items-center justify-between text-xs">
            <span className={state.error ? 'text-[var(--danger)]' : 'muted'}>
              {state.error ?? state.status}
            </span>
            {state.active && <span className="muted tabular-nums">{state.percent}%</span>}
            {state.done && <span className="text-[var(--ok)]">✓ Done</span>}
          </div>
          {(state.active || state.done) && (
            <div className="rf-progress-track">
              <div
                className="rf-progress-fill transition-all duration-300"
                style={{ width: `${state.percent}%` }}
              />
            </div>
          )}
        </div>
      )}

      {/* Link to GGUF catalog */}
      <p className="text-[0.65rem] muted">
        Browse GGUF models on{' '}
        <a
          href="https://huggingface.co/models?library=gguf"
          target="_blank"
          rel="noopener noreferrer"
          className="text-[var(--purple)] hover:underline"
        >
          Hugging Face
        </a>
      </p>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Installed models list section
// ---------------------------------------------------------------------------
function InstalledModels({
  models,
  selectedModel,
  onSelect,
  onDeleted,
}: {
  models: AiModel[];
  selectedModel: string;
  onSelect: (name: string) => void;
  onDeleted: (name: string) => void;
}) {
  const [deleting, setDeleting] = useState<string | null>(null);

  const handleDelete = async (name: string) => {
    if (deleting) return;
    setDeleting(name);
    try {
      await deleteModel(name);
      onDeleted(name);
    } catch {
      // silently ignore for now
    } finally {
      setDeleting(null);
    }
  };

  if (models.length === 0) {
    return (
      <div className="space-y-2">
        <p className="text-xs font-semibold text-[var(--text-muted)] uppercase tracking-wider">Installed Models</p>
        <p className="text-xs muted">No models installed. Pull one above.</p>
      </div>
    );
  }

  return (
    <div className="space-y-2">
      <p className="text-xs font-semibold text-[var(--text-muted)] uppercase tracking-wider">
        Installed Models <span className="normal-case font-normal">({models.length})</span>
      </p>
      <div className="space-y-1.5">
        {models.map((m) => {
          const isSelected = m.name === selectedModel;
          const isDeleting = deleting === m.name;
          return (
            <div
              key={m.name}
              className="flex items-center gap-2 px-3 py-2 rounded-xl transition-all duration-150 cursor-pointer"
              style={{
                background: isSelected ? 'rgba(177,140,255,0.1)' : 'rgba(255,255,255,0.03)',
                border: isSelected ? '1px solid rgba(177,140,255,0.3)' : '1px solid var(--border)',
              }}
              onClick={() => onSelect(m.name)}
            >
              {/* Active dot */}
              <span
                className="w-2 h-2 rounded-full flex-shrink-0 transition-all"
                style={{ background: isSelected ? 'var(--purple)' : 'transparent', border: isSelected ? 'none' : '1px solid var(--border)' }}
              />

              <div className="flex-1 min-w-0">
                <p className="text-xs font-mono truncate">{m.name}</p>
                <p className="text-[0.65rem] muted">
                  {[m.parameter_size, m.quantization, formatGb(m.size_gb)]
                    .filter(Boolean)
                    .join(' · ')}
                </p>
              </div>

              <button
                onClick={(e) => { e.stopPropagation(); handleDelete(m.name); }}
                disabled={isDeleting}
                className="flex-shrink-0 w-6 h-6 flex items-center justify-center rounded-lg muted hover:text-[var(--danger)] transition-colors disabled:opacity-40"
                title="Delete model"
              >
                {isDeleting ? (
                  <span className="w-3 h-3 border border-current border-t-transparent rounded-full animate-spin" />
                ) : (
                  <svg className="w-3.5 h-3.5" viewBox="0 0 16 16" fill="currentColor">
                    <path d="M6 2h4a1 1 0 0 1 1 1v1H5V3a1 1 0 0 1 1-1zm-3 3h10l-1 9H4L3 5zm3 2v5h1V7H6zm3 0v5h1V7H9z" />
                  </svg>
                )}
              </button>
            </div>
          );
        })}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Model management side panel
// ---------------------------------------------------------------------------
function ManagementPanel({
  models,
  selectedModel,
  onSelect,
  onClose,
  onModelsChanged,
}: {
  models: AiModel[];
  selectedModel: string;
  onSelect: (name: string) => void;
  onClose: () => void;
  onModelsChanged: () => void;
}) {
  const [gpuData, setGpuData] = useState<GpusResponse | null>(null);
  const [running, setRunning] = useState<RunningModel[]>([]);
  const [localModels, setLocalModels] = useState<AiModel[]>(models);

  useEffect(() => {
    setLocalModels(models);
  }, [models]);

  useEffect(() => {
    fetchGpus().then(setGpuData).catch(() => {});
    fetchRunningModels().then(setRunning).catch(() => {});
    // Refresh running models every 8s while panel is open
    const id = setInterval(() => {
      fetchRunningModels().then(setRunning).catch(() => {});
    }, 8000);
    return () => clearInterval(id);
  }, []);

  const handleDeleted = (name: string) => {
    setLocalModels((prev) => prev.filter((m) => m.name !== name));
    onModelsChanged();
  };

  const handlePulled = () => {
    onModelsChanged();
    fetchModels()
      .then((res) => setLocalModels(res.models))
      .catch(() => {});
  };

  return (
    <div
      className="flex flex-col flex-shrink-0 w-72 overflow-y-auto"
      style={{
        background: 'rgba(30,36,54,0.98)',
        borderLeft: '1px solid var(--border)',
        borderRadius: '0 var(--radius-lg) var(--radius-lg) 0',
      }}
    >
      {/* Panel header */}
      <div className="flex items-center justify-between px-4 py-3 border-b border-[var(--border)]">
        <span className="text-sm font-semibold">Models & GPU</span>
        <button
          onClick={onClose}
          className="w-7 h-7 flex items-center justify-center rounded-lg muted hover:text-[var(--text-main)] transition-colors"
        >
          <svg className="w-4 h-4" viewBox="0 0 16 16" fill="currentColor">
            <path d="M3.72 3.72a.75.75 0 0 1 1.06 0L8 6.94l3.22-3.22a.75.75 0 1 1 1.06 1.06L9.06 8l3.22 3.22a.75.75 0 1 1-1.06 1.06L8 9.06l-3.22 3.22a.75.75 0 0 1-1.06-1.06L6.94 8 3.72 4.78a.75.75 0 0 1 0-1.06z" />
          </svg>
        </button>
      </div>

      {/* Scrollable body */}
      <div className="flex-1 overflow-y-auto px-4 py-4 space-y-6">
        <PullSection onPulled={handlePulled} />

        <div className="border-t border-[var(--border)]" />

        <InstalledModels
          models={localModels}
          selectedModel={selectedModel}
          onSelect={(name) => { onSelect(name); }}
          onDeleted={handleDeleted}
        />

        <div className="border-t border-[var(--border)]" />

        {gpuData && (
          <GpuSection
            gpus={gpuData.gpus}
            gpuNote={gpuData.multi_gpu_note}
            cudaEnv={gpuData.cuda_visible_devices}
            running={running}
          />
        )}
      </div>
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
function InferenceUnavailable({ onOpenModels }: { onOpenModels: () => void }) {
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
        <p className="font-semibold">No model loaded</p>
        <p className="text-sm muted max-w-xs">
          Open the Models panel and pull a model to get started.
        </p>
      </div>
      <button onClick={onOpenModels} className="btn-primary px-3 py-1.5 text-xs rounded-xl">
        Open Models panel
      </button>
      <span className="chip chip-accent text-[0.7rem]">
        Recommended: llama3.2:3b (~2 GB)
      </span>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Empty / suggestions state
// ---------------------------------------------------------------------------
function EmptyState({ model, onSuggest }: { model: string; onSuggest: (s: string) => void }) {
  const suggestions = [
    "What's in the media library?",
    "Who has a birthday coming up?",
    "Create a watch party and invite everyone",
    "What rooms are active right now?",
  ];

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
          I know about your media, rooms, calendar, and more.
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
  const [selectedModel, setSelectedModel] = useState('');
  const [messages, setMessages] = useState<ChatEntry[]>([]);
  const [input, setInput] = useState('');
  const [isStreaming, setIsStreaming] = useState(false);
  const [panelOpen, setPanelOpen] = useState(false);

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
        setModels(res.models);
        if (res.models.length > 0 && !selectedModel) {
          setSelectedModel(res.models[0].name);
        } else if (res.models.length === 0) {
          setSelectedModel('');
        }
      })
      .catch(() => setInferenceAvailable(false));
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
      isStreaming: false, stats: null, error: null,
    };
    const assistantId = uid();
    const assistantEntry: ChatEntry = {
      id: assistantId, role: 'assistant', content: '',
      isStreaming: true, stats: null, error: null,
    };

    setMessages((prev) => [...prev, userEntry, assistantEntry]);
    setInput('');
    if (textareaRef.current) textareaRef.current.style.height = 'auto';
    setIsStreaming(true);

    const history: ChatHistoryMessage[] = messages.map((m) => ({ role: m.role, content: m.content }));

    stopRef.current = streamChat(
      selectedModel, text, history,
      (event) => {
        if (event.type === 'token') {
          setMessages((prev) =>
            prev.map((m) => m.id === assistantId ? { ...m, content: m.content + event.text } : m),
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
            <button
              onClick={() => setPanelOpen((o) => !o)}
              className={`px-3 py-1.5 text-xs rounded-xl border transition-colors flex items-center gap-1.5 ${
                panelOpen
                  ? 'border-[var(--purple)] text-[var(--purple)]'
                  : 'border-[var(--border)] muted hover:text-[var(--text-main)]'
              }`}
            >
              <svg className="w-3.5 h-3.5" viewBox="0 0 16 16" fill="currentColor">
                <path d="M2 4.5A.5.5 0 0 1 2.5 4h11a.5.5 0 0 1 0 1h-11a.5.5 0 0 1-.5-.5zm0 4A.5.5 0 0 1 2.5 8h6a.5.5 0 0 1 0 1h-6a.5.5 0 0 1-.5-.5zm0 4a.5.5 0 0 1 .5-.5h4a.5.5 0 0 1 0 1h-4a.5.5 0 0 1-.5-.5z" />
              </svg>
              Models
            </button>
          </div>
        </div>

        {/* ── Body row (chat + optional panel) ───────────────────────────── */}
        <div className="panel flex-1 flex overflow-hidden">

          {/* Chat area */}
          <div className="flex flex-col flex-1 overflow-hidden">
            {inferenceAvailable === false && (
              <InferenceUnavailable onOpenModels={() => setPanelOpen(true)} />
            )}
            {inferenceAvailable === null && (
              <div className="flex-1 flex items-center justify-center">
                <p className="text-sm muted">Loading…</p>
              </div>
            )}
            {inferenceAvailable === true && !selectedModel && (
              <InferenceUnavailable onOpenModels={() => setPanelOpen(true)} />
            )}

            {inferenceAvailable === true && selectedModel && (
              <>
                <div ref={threadRef} className="flex-1 overflow-y-auto px-5 py-5 space-y-5">
                  {messages.length === 0 && (
                    <EmptyState model={selectedModel} onSuggest={(s) => setInput(s)} />
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

          {/* Management panel */}
          {panelOpen && (
            <ManagementPanel
              models={models}
              selectedModel={selectedModel}
              onSelect={(name) => { setSelectedModel(name); }}
              onClose={() => setPanelOpen(false)}
              onModelsChanged={loadModels}
            />
          )}
        </div>
      </div>
    </>
  );
}
