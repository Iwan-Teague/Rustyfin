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
  const [msgType, setMsgType] = useState<'ok' | 'error'>('ok');

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
      setMsgType('ok');
      setMsg('Library created');
      setNewLib({ name: '', kind: 'movies', path: '' });
      loadData();
    } catch (err: any) {
      setMsgType('error');
      setMsg(err.message);
    }
  }

  async function scanLibrary(libId: string) {
    try {
      await apiFetch(`/libraries/${libId}/scan`, { method: 'POST' });
      setMsgType('ok');
      setMsg('Scan started');
      loadData();
    } catch (err: any) {
      setMsgType('error');
      setMsg(err.message);
    }
  }

  return (
    <div className="space-y-8 animate-rise">
      <header className="space-y-2">
        <span className="chip">Server Controls</span>
        <h1 className="text-3xl font-semibold sm:text-4xl">Admin Dashboard</h1>
        <p className="text-sm muted sm:text-base">
          Configure media sources, trigger scans, and monitor active processing jobs.
        </p>
      </header>

      {msg && (
        <p className={`${msgType === 'ok' ? 'notice-ok' : 'notice-error'} rounded-xl px-4 py-2 text-sm`}>
          {msg}
        </p>
      )}

      {/* Create Library */}
      <section className="panel space-y-4 p-6">
        <h2 className="text-xl font-semibold">Create Library</h2>
        <form onSubmit={createLibrary} className="grid grid-cols-1 gap-3 md:grid-cols-[1.1fr_0.9fr_2fr_auto]">
          <input
            placeholder="Name"
            value={newLib.name}
            onChange={(e) => setNewLib({ ...newLib, name: e.target.value })}
            className="input px-3 py-2 text-sm"
            required
          />
          <select
            value={newLib.kind}
            onChange={(e) => setNewLib({ ...newLib, kind: e.target.value })}
            className="select px-3 py-2 text-sm"
          >
            <option value="movies">Movies</option>
            <option value="tv_shows">TV Shows</option>
          </select>
          <input
            placeholder="/path/to/media"
            value={newLib.path}
            onChange={(e) => setNewLib({ ...newLib, path: e.target.value })}
            className="input px-3 py-2 text-sm"
            required
          />
          <button
            type="submit"
            className="btn-primary px-4 py-2 text-sm"
          >
            Create
          </button>
        </form>
      </section>

      {/* Libraries */}
      <section className="space-y-3">
        <h2 className="text-xl font-semibold">Libraries</h2>
        {libraries.length === 0 ? (
          <div className="panel-soft px-4 py-3">
            <p className="text-sm muted">No libraries configured.</p>
          </div>
        ) : (
          libraries.map((lib) => (
            <div
              key={lib.id}
              className="tile flex items-center justify-between gap-4 p-4"
            >
              <div>
                <p className="font-medium">{lib.name}</p>
                <p className="text-sm muted">{lib.kind} · {lib.item_count} items</p>
              </div>
              <button
                onClick={() => scanLibrary(lib.id)}
                className="btn-secondary px-3 py-1.5 text-sm"
              >
                Scan
              </button>
            </div>
          ))
        )}
      </section>

      {/* Jobs */}
      <section className="space-y-3">
        <h2 className="text-xl font-semibold">Jobs</h2>
        {jobs.length === 0 ? (
          <p className="text-sm muted">No active jobs</p>
        ) : (
          jobs.map((job) => (
            <div
              key={job.id}
              className="tile space-y-2 p-3"
            >
              <div className="flex items-center justify-between gap-3">
                <p className="text-sm font-medium">{job.kind}</p>
                <span className="chip">{job.status}</span>
              </div>
              <p className="text-xs muted">{Math.round(job.progress * 100)}%</p>
              <div className="h-2 overflow-hidden rounded-full bg-white/8">
                <div
                  className="h-full rounded-full bg-gradient-to-r from-[var(--orange)] to-[var(--purple)]"
                  style={{ width: `${Math.max(0, Math.min(100, Math.round(job.progress * 100)))}%` }}
                />
              </div>
            </div>
          ))
        )}
      </section>
    </div>
  );
}
