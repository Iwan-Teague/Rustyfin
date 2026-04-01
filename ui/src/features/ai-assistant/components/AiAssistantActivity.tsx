import { type AiActivityTraceItem } from '@/lib/aiApi';

function phaseDetailLabel(phase: 'planning' | 'generating'): string {
  return phase === 'planning' ? 'Planning response' : 'Generating answer';
}

export default function AiAssistantActivity({
  activityTrace,
  isStreaming,
}: {
  activityTrace: AiActivityTraceItem[];
  isStreaming: boolean;
}) {
  const phaseItems = activityTrace.filter((item) => item.kind === 'phase');
  const toolItems = activityTrace.filter((item) => item.kind === 'tool');
  const activePhase = [...phaseItems].reverse().find((item) => !item.finished_ts_ms) ?? null;
  const latestPhase = phaseItems.at(-1) ?? null;
  const showThinking = phaseItems.length > 0 || isStreaming;

  if (!showThinking && toolItems.length === 0) {
    return null;
  }

  return (
    <div className="mb-3 space-y-2.5">
      {showThinking && (
        <div
          className="ai-thinking-row"
          data-active={activePhase ? 'true' : 'false'}
        >
          <span className="ai-thinking-dot" aria-hidden="true" />
          <div className="min-w-0">
            <div className="text-[0.76rem] font-medium text-[var(--text-main)]">
              Thinking...
            </div>
            {latestPhase && (
              <div className="text-[0.65rem] muted">
                {phaseDetailLabel(latestPhase.phase)}
              </div>
            )}
          </div>
          {activePhase && <span className="ai-thinking-sweep" aria-hidden="true" />}
        </div>
      )}

      {toolItems.map((item) => (
        <div
          key={item.id}
          className="ai-tool-call-row"
          data-state={item.state}
        >
          <span className="ai-tool-call-indicator" aria-hidden="true" />
          <div className="min-w-0">
            <div className="text-[0.72rem] text-[var(--text-main)]">
              {item.label}
            </div>
            <div className="text-[0.62rem] font-mono muted">{item.tool}</div>
          </div>
        </div>
      ))}
    </div>
  );
}
