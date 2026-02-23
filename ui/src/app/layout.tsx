import type { Metadata, Viewport } from 'next';
import './globals.css';
import Providers from './providers';
import NavBar from './NavBar';
import MiniPlayer from './components/MiniPlayer';

export const metadata: Metadata = {
  title: 'Rustfin',
  description: 'Local-first media server',
};

export const viewport: Viewport = {
  width: 'device-width',
  initialScale: 1,
  maximumScale: 1,
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <body className="min-h-screen text-[var(--text-main)]">
        <Providers>
          <div className="mx-auto max-w-[90rem] px-4 pb-24 pt-5 sm:px-6 lg:px-10">
            <NavBar />
            <main className="mx-auto max-w-7xl px-0 py-4 sm:py-8 lg:py-10">{children}</main>
            <footer className="mt-4 px-1 text-xs muted">
              Local-first media, styled for modern home servers.
            </footer>
          </div>
          <MiniPlayer />
        </Providers>
      </body>
    </html>
  );
}
