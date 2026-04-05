'use client';

type Props = {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  className?: string;
  disabled?: boolean;
};

export default function RfSwitch({
  label,
  checked,
  onChange,
  className,
  disabled = false,
}: Props) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      data-checked={checked}
      disabled={disabled}
      className={['rf-vault-switch disabled:cursor-not-allowed disabled:opacity-60', className]
        .filter(Boolean)
        .join(' ')}
      onClick={() => onChange(!checked)}
    >
      <span className="rf-vault-switch-track" aria-hidden="true">
        <span className="rf-vault-switch-state">{checked ? 'ON' : 'OFF'}</span>
        <span className="rf-vault-switch-thumb" />
      </span>
      <span className="rf-vault-switch-label">{label}</span>
    </button>
  );
}
