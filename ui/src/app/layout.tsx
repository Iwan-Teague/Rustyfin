import type { Metadata } from 'next';
import './globals.css';

export const metadata: Metadata = {
  title: 'Rustfin',
  description: 'Local-first media server',
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en" className="dark">
      <body className="bg-gray-950 text-gray-100 min-h-screen">
        <nav className="border-b border-gray-800 px-6 py-3 flex items-center gap-6">
          <a href="/" className="text-xl font-bold text-blue-400">Rustfin</a>
          <a href="/libraries" className="text-gray-300 hover:text-white">Libraries</a>
          <a href="/admin" className="text-gray-300 hover:text-white">Admin</a>
          <div className="ml-auto">
            <a href="/login" className="text-gray-400 hover:text-white text-sm">Login</a>
          </div>
        </nav>
        <main className="max-w-7xl mx-auto px-6 py-8">{children}</main>
      </body>
    </html>
  );
}
