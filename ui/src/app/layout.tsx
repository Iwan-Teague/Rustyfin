import type { Metadata } from 'next';
import './globals.css';
import Providers from './providers';
import NavBar from './NavBar';

export const metadata: Metadata = {
  title: 'Rustfin',
  description: 'Local-first media server',
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <body className="min-h-screen text-[var(--text-main)]">
        <Providers>
          <div className="mx-auto max-w-[90rem] px-4 pb-8 pt-5 sm:px-6 lg:px-10">
            <NavBar />
            <main className="mx-auto max-w-7xl px-0 py-8 sm:py-10">{children}</main>
            <footer className="mt-4 px-1 text-xs muted">
              Local-first media, styled for modern home servers.
            </footer>
          </div>
        </Providers>
      </body>
    </html>
  );
}
