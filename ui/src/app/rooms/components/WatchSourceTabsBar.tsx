type WatchSource = 'video' | 'youtube' | 'web';

type Props = {
  activeSource: WatchSource;
  onSwitchSource: (source: WatchSource) => void;
  switchingDisabled: boolean;
  badges: string[];
  className?: string;
};

const WATCH_SOURCE_OPTIONS: Array<{ source: WatchSource; label: string }> = [
  { source: 'video', label: 'Local Media' },
  { source: 'youtube', label: 'YouTube' },
  { source: 'web', label: 'Web' },
];

export default function WatchSourceTabsBar({
  activeSource,
  onSwitchSource,
  switchingDisabled,
  badges,
  className,
}: Props) {
  return (
    <div className={className}>
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex flex-wrap items-end gap-2">
          {WATCH_SOURCE_OPTIONS.map(({ source, label }) => (
            <button
              key={source}
              type="button"
              onClick={() => onSwitchSource(source)}
              disabled={switchingDisabled || activeSource === source}
              className={`px-5 py-2.5 text-sm font-medium rounded-t-lg transition-colors disabled:opacity-60 ${
                activeSource === source
                  ? 'bg-[var(--surface)] border border-b-0 border-[var(--border)]'
                  : 'opacity-60 hover:opacity-100 hover:bg-[var(--surface)] hover:bg-opacity-50 hover:border hover:border-b-0 hover:border-[var(--border)] hover:border-opacity-50'
              }`}
            >
              {label}
            </button>
          ))}
        </div>
        <div className="ml-auto flex flex-wrap items-center justify-end gap-2">
          {badges.map((badge) => (
            <span key={badge} className="chip">
              {badge}
            </span>
          ))}
        </div>
      </div>
    </div>
  );
}
