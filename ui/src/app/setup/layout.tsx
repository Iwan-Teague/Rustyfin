export default function SetupLayout({ children }: { children: React.ReactNode }) {
  return (
    <div className="mx-auto w-full max-w-3xl animate-rise space-y-6">
      <header className="panel px-6 py-5 sm:px-8">
        <div className="space-y-2">
          <span className="chip chip-accent">First-Time Experience</span>
          <h1 className="text-3xl font-semibold sm:text-4xl">
            <span className="accent-logo">Rustyfin</span> Setup
          </h1>
          <p className="text-sm muted sm:text-base">
            Configure your server in a few guided steps, then start streaming.
          </p>
        </div>
      </header>
      <main>{children}</main>
    </div>
  );
}
