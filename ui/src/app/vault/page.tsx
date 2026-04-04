import dynamic from 'next/dynamic';

const RustyVaultPage = dynamic(() => import('@/features/rustyvault/RustyVaultPage'));

const rustyvaultEnabled =
  process.env.NEXT_PUBLIC_RUSTYVAULT_ENABLED !== '0' &&
  process.env.RUSTFIN_RUSTYVAULT_ENABLED !== '0';

export default function VaultPage() {
  if (!rustyvaultEnabled) {
    return (
      <div className="rf-flat-empty animate-rise px-5 py-4">
        <h1 className="text-lg font-semibold text-slate-100">Vault unavailable</h1>
        <p className="mt-2 text-sm text-slate-300">
          RustyVault is currently disabled on this host. Ask an administrator to enable
          `RUSTFIN_RUSTYVAULT_ENABLED` and restart the runtime when Vault access is needed.
        </p>
      </div>
    );
  }

  return <RustyVaultPage />;
}
