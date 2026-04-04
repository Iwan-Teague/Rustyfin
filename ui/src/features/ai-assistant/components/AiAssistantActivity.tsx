import { type AiActivityTraceItem } from '@/lib/aiApi';

function phaseDetailLabel(phase: 'planning' | 'generating'): string {
  return phase === 'planning' ? 'Planning response' : 'Generating answer';
}

function formatThoughtDurationSeconds(
  startedTsMs: number,
  finishedTsMs?: number | null,
): string | undefined {
  if (!finishedTsMs || finishedTsMs <= startedTsMs) {
    return undefined;
  }

  const durationSeconds = Math.max(1, Math.round((finishedTsMs - startedTsMs) / 1000));
  return `${durationSeconds}s`;
}

type ActivityRow = {
  key: string;
  primary: string;
  meta?: string;
  state: 'running' | 'complete' | 'error';
  active: boolean;
};

export default function AiAssistantActivity({
  activityTrace,
  isStreaming,
}: {
  activityTrace: AiActivityTraceItem[];
  isStreaming: boolean;
}) {
  const rows: ActivityRow[] = activityTrace.map((item) => {
    if (item.kind === 'phase') {
      const durationLabel = formatThoughtDurationSeconds(
        item.started_ts_ms,
        item.finished_ts_ms,
      );
      const isComplete = Boolean(item.finished_ts_ms);
      return {
        key: `phase-${item.phase}-${item.started_ts_ms}`,
        primary: isComplete ? 'Thought' : item.label?.trim() || phaseDetailLabel(item.phase),
        meta: isComplete ? durationLabel : undefined,
        state: isComplete ? 'complete' : 'running',
        active: !isComplete,
      };
    }

    return {
      key: `tool-${item.id}`,
      primary: item.label,
      meta: item.tool,
      state: item.state,
      active: item.state === 'running',
    };
  });

  if (rows.length === 0 && isStreaming) {
    rows.push({
      key: 'streaming-thinking',
      primary: 'Thinking',
      state: 'running',
      active: true,
    });
  }

  if (rows.length === 0) {
    return null;
  }

  return (
    <div className="ai-activity-stream mb-3" role="status" aria-live="polite">
      {rows.map((row) => (
        <div
          key={row.key}
          className="ai-activity-row-inline"
          data-active={row.active ? 'true' : 'false'}
          data-state={row.state}
        >
          <div className="ai-activity-line">
            <span
              className="ai-activity-primary"
              data-active={row.active ? 'true' : 'false'}
              data-text={row.primary}
            >
              {row.primary}
            </span>
            {row.meta ? <span className="ai-activity-meta">{row.meta}</span> : null}
          </div>
        </div>
      ))}
    </div>
  );
}
