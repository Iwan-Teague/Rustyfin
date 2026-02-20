export default function SetupLayout({ children }: { children: React.ReactNode }) {
  return (
    <div className="min-h-screen bg-gray-950 text-gray-100 flex flex-col">
      <header className="border-b border-gray-800 px-6 py-4">
        <h1 className="text-xl font-bold text-blue-400">Rustyfin Setup</h1>
      </header>
      <main className="flex-1 max-w-2xl mx-auto w-full px-6 py-8">{children}</main>
    </div>
  );
}
