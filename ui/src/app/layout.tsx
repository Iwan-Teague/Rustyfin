import type { Metadata } from 'next';
import './globals.css';

export const metadata: Metadata = {
  title: 'Rustfin',
  description: 'Local-first media server',
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <body className="min-h-screen text-[var(--text-main)]">
        <div className="mx-auto max-w-[90rem] px-4 pb-8 pt-5 sm:px-6 lg:px-10">
          <nav className="app-nav animate-rise rounded-2xl px-4 py-3 sm:px-6">
            <div className="flex items-center gap-3 sm:gap-5">
              <a href="/" className="text-2xl font-semibold accent-logo">Rustyfin</a>
              <span className="chip chip-accent hidden md:inline-flex">Home Server Streaming</span>
              <a href="/libraries" className="btn-ghost px-3 py-2 text-sm sm:text-base">Libraries</a>
              <a href="/admin" className="btn-ghost px-3 py-2 text-sm sm:text-base">Admin</a>
              <div className="ml-auto">
                <a href="/login" className="btn-secondary px-4 py-2 text-sm">Login</a>
              </div>
            </div>
          </nav>
          <main className="mx-auto max-w-7xl px-0 py-8 sm:py-10">{children}</main>
          <footer className="mt-4 px-1 text-xs muted">
            Local-first media, styled for modern home servers.
          </footer>
        </div>
      </body>
    </html>
  );
}
