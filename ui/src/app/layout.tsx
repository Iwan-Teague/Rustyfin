import type { Metadata, Viewport } from 'next';
import './globals.css';
import Providers from './providers';
import NavBar from './NavBar';
import MiniPlayer from './components/MiniPlayer';
import PrimaryButtonEffects from './components/PrimaryButtonEffects';

export const metadata: Metadata = {
  title: 'Rustfin',
  description: 'Local-first media server',
};

export const viewport: Viewport = {
  width: 'device-width',
  initialScale: 1,
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <body className="min-h-screen text-[var(--text-main)]">
        <Providers>
          <PrimaryButtonEffects />
          <div className="w-full px-4 pb-20 pt-5 sm:px-6 sm:pb-24 lg:px-10">
            <NavBar />
            <main className="mx-auto max-w-7xl px-0 py-4 sm:py-8 lg:py-10">{children}</main>
          </div>
          <MiniPlayer />
        </Providers>
      </body>
    </html>
  );
}
