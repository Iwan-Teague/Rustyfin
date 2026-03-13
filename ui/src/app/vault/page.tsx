import dynamic from 'next/dynamic';

const RustyVaultPage = dynamic(() => import('@/features/rustyvault/RustyVaultPage'));

const rustyvaultEnabled =
  process.env.NEXT_PUBLIC_RUSTYVAULT_ENABLED !== '0' &&
  process.env.RUSTFIN_RUSTYVAULT_ENABLED !== '0';

export default function VaultPage() {
  if (!rustyvaultEnabled) {
    return (
      <div className="panel-soft animate-rise px-5 py-4">
        <p className="text-sm muted">Vault is unavailable on this host.</p>
      </div>
    );
  }

  return <RustyVaultPage />;
}
