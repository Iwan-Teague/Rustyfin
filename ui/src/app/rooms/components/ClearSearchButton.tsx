type Props = {
  onClick: () => void;
  className?: string;
  title?: string;
  ariaLabel?: string;
};

export default function ClearSearchButton({
  onClick,
  className = '',
  title = 'Clear search',
  ariaLabel = 'Clear search',
}: Props) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`inline-flex h-6 w-6 items-center justify-center rounded-full border border-white/25 text-white/75 transition hover:border-white/50 hover:text-white ${className}`.trim()}
      aria-label={ariaLabel}
      title={title}
    >
      <svg viewBox="0 0 24 24" className="h-4 w-4" fill="none" aria-hidden="true">
        <circle cx="12" cy="12" r="8" stroke="currentColor" strokeWidth="1.8" />
        <path
          d="M9 9l6 6M15 9l-6 6"
          stroke="currentColor"
          strokeWidth="1.8"
          strokeLinecap="round"
        />
      </svg>
    </button>
  );
}
