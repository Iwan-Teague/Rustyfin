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
  badgesClassName?: string;
  className?: string;
  variant?: 'surface' | 'flat' | 'vault';
};

export default function SurfaceTabsBar<T extends string>({
  activeKey,
  onSelect,
  disabled = false,
  options,
  badges = [],
  badgesClassName = '',
  className,
  variant = 'surface',
}: Props<T>) {
  return (
    <div className={className}>
      <div className="grid grid-cols-[auto_minmax(0,1fr)] items-start gap-x-3 gap-y-2">
        <div className="flex flex-wrap items-end gap-2 self-start">
          {options.map(({ key, label }) => (
            <button
              key={key}
              type="button"
              onClick={() => onSelect(key)}
              disabled={disabled || activeKey === key}
              className={
                variant === 'flat'
                  ? `rounded-none border-b-2 px-0 py-1.5 text-sm font-medium transition-colors disabled:opacity-60 ${
                      activeKey === key
                        ? 'border-[var(--orange-soft)] text-white'
                        : 'border-transparent text-white/62 hover:border-white/16 hover:text-white'
                    }`
                  : variant === 'vault'
                    ? `relative rounded-none px-0 py-1.5 text-sm font-medium transition-colors disabled:opacity-60 ${
                        activeKey === key
                          ? 'text-white after:absolute after:bottom-0 after:left-0 after:right-0 after:h-px after:bg-[rgba(255,255,255,0.55)]'
                          : 'text-white/58 hover:text-white'
                      }`
                  : `rounded-t-lg px-5 py-2.5 text-sm font-medium transition-colors disabled:opacity-60 ${
                      activeKey === key
                        ? 'border border-[var(--border)] border-b-0 bg-[var(--surface)]'
                        : 'opacity-60 hover:border hover:border-[var(--border)] hover:border-b-0 hover:border-opacity-50 hover:bg-[var(--surface)] hover:bg-opacity-50 hover:opacity-100'
                    }`
              }
            >
              {label}
            </button>
          ))}
        </div>
        <div
          className={`min-w-0 flex flex-wrap items-center justify-end gap-2 self-start ${badgesClassName}`.trim()}
        >
          {badges.map((badge) => (
            <span
              key={badge}
              className={
                variant === 'flat' || variant === 'vault'
                  ? 'text-xs text-white/58'
                  : 'chip'
              }
            >
              {badge}
            </span>
          ))}
        </div>
      </div>
    </div>
  );
}
