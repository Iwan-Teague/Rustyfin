'use client';

type Props = {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  className?: string;
};

export default function RfSwitch({ label, checked, onChange, className }: Props) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      data-checked={checked}
      className={['rf-vault-switch', className].filter(Boolean).join(' ')}
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
