'use client';

import { useEffect, useState } from 'react';
import { apiJson, apiFetch } from '@/lib/api';

interface Library {
  id: string;
  name: string;
  kind: string;
  item_count: number;
}

interface Job {
  id: string;
  kind: string;
  status: string;
  progress: number;
}

export default function AdminPage() {
  const [libraries, setLibraries] = useState<Library[]>([]);
  const [jobs, setJobs] = useState<Job[]>([]);
  const [newLib, setNewLib] = useState({ name: '', kind: 'movies', path: '' });
  const [msg, setMsg] = useState('');

  useEffect(() => {
    loadData();
  }, []);

  async function loadData() {
    try {
      const libs = await apiJson<Library[]>('/libraries');
      setLibraries(libs);
      const jobList = await apiJson<Job[]>('/jobs');
      setJobs(jobList);
    } catch {}
  }

  async function createLibrary(e: React.FormEvent) {
    e.preventDefault();
    try {
      await apiFetch('/libraries', {
        method: 'POST',
        body: JSON.stringify({
          name: newLib.name,
          kind: newLib.kind,
          paths: [newLib.path],
        }),
      });
      setMsg('Library created');
      setNewLib({ name: '', kind: 'movies', path: '' });
      loadData();
    } catch (err: any) {
      setMsg(err.message);
    }
  }

  async function scanLibrary(libId: string) {
    try {
      await apiFetch(`/libraries/${libId}/scan`, { method: 'POST' });
      setMsg('Scan started');
      loadData();
    } catch (err: any) {
      setMsg(err.message);
    }
  }

  return (
    <div className="space-y-8">
      <h1 className="text-2xl font-bold">Admin Dashboard</h1>

      {msg && (
        <p className="text-sm text-green-400 bg-green-900/20 px-4 py-2 rounded">{msg}</p>
      )}

      {/* Create Library */}
      <section className="space-y-4">
        <h2 className="text-xl font-semibold">Create Library</h2>
        <form onSubmit={createLibrary} className="flex gap-3 flex-wrap">
          <input
            placeholder="Name"
            value={newLib.name}
            onChange={(e) => setNewLib({ ...newLib, name: e.target.value })}
            className="px-3 py-2 bg-gray-900 border border-gray-700 rounded text-sm"
            required
          />
          <select
            value={newLib.kind}
            onChange={(e) => setNewLib({ ...newLib, kind: e.target.value })}
            className="px-3 py-2 bg-gray-900 border border-gray-700 rounded text-sm"
          >
            <option value="movies">Movies</option>
            <option value="tv_shows">TV Shows</option>
          </select>
          <input
            placeholder="/path/to/media"
            value={newLib.path}
            onChange={(e) => setNewLib({ ...newLib, path: e.target.value })}
            className="px-3 py-2 bg-gray-900 border border-gray-700 rounded text-sm flex-1"
            required
          />
          <button
            type="submit"
            className="px-4 py-2 bg-blue-600 hover:bg-blue-700 rounded text-sm font-semibold"
          >
            Create
          </button>
        </form>
      </section>

      {/* Libraries */}
      <section className="space-y-3">
        <h2 className="text-xl font-semibold">Libraries</h2>
        {libraries.map((lib) => (
          <div
            key={lib.id}
            className="flex items-center justify-between p-4 bg-gray-900 rounded border border-gray-800"
          >
            <div>
              <p className="font-medium">{lib.name}</p>
              <p className="text-sm text-gray-400">{lib.kind} · {lib.item_count} items</p>
            </div>
            <button
              onClick={() => scanLibrary(lib.id)}
              className="px-3 py-1 bg-gray-700 hover:bg-gray-600 rounded text-sm"
            >
              Scan
            </button>
          </div>
        ))}
      </section>

      {/* Jobs */}
      <section className="space-y-3">
        <h2 className="text-xl font-semibold">Jobs</h2>
        {jobs.length === 0 ? (
          <p className="text-gray-400 text-sm">No active jobs</p>
        ) : (
          jobs.map((job) => (
            <div
              key={job.id}
              className="flex items-center justify-between p-3 bg-gray-900 rounded border border-gray-800"
            >
              <div>
                <p className="text-sm font-medium">{job.kind}</p>
                <p className="text-xs text-gray-400">
                  {job.status} · {Math.round(job.progress * 100)}%
                </p>
              </div>
            </div>
          ))
        )}
      </section>
    </div>
  );
}
