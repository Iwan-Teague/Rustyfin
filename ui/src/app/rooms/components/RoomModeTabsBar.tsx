type TabOption<T extends string> = {
  key: T;
  label: string;
};

type Props<T extends string> = {
  activeKey: T;
  onSelect: (key: T) => void;
  disabled?: boolean;
  options: TabOption<T>[];
  badges?: string[];
  className?: string;
};

export default function RoomModeTabsBar<T extends string>({
  activeKey,
  onSelect,
  disabled = false,
  options,
  badges = [],
  className,
}: Props<T>) {
  return (
    <div className={className}>
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex flex-wrap items-end gap-2">
          {options.map(({ key, label }) => (
            <button
              key={key}
              type="button"
              onClick={() => onSelect(key)}
              disabled={disabled || activeKey === key}
              className={`px-5 py-2.5 text-sm font-medium rounded-t-lg transition-colors disabled:opacity-60 ${
                activeKey === key
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
