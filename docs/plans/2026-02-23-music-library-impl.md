# Music Library Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add music library support — admins can create a music library, the scanner detects audio files and builds an artist/album/track hierarchy, and users can browse and play tracks via a persistent mini-player bar.

**Architecture:** The scanner already handles music libraries end-to-end. What's missing is (a) backend validation blocking `music` as a library kind, (b) `duration_ms` missing from item responses, and (c) all frontend browsing/playback UI. The existing `/items/{id}/playback` endpoint already issues a `direct_url` that works for audio — no new streaming routes needed. A React context holds audio state and a hidden `<audio>` element; a fixed bottom bar is the player UI.

**Tech Stack:** Rust/Axum/SQLx (backend), Next.js App Router/TypeScript (frontend), HTML5 `<audio>` API (playback)

---

## Task 1: Allow `music` kind in `create_library`

**Files:**
- Modify: `crates/server/src/routes.rs:668-669`

**Step 1: Change the validation**

In `routes.rs`, find the `create_library` handler at line 668. Change:

```rust
if body.kind != "movies" && body.kind != "tv_shows" {
    return Err(ApiError::BadRequest("kind must be 'movies' or 'tv_shows'".into()).into());
}
```

to:

```rust
if body.kind != "movies" && body.kind != "tv_shows" && body.kind != "music" {
    return Err(ApiError::BadRequest("kind must be 'movies', 'tv_shows', or 'music'".into()).into());
}
```

**Step 2: Verify it compiles**

```bash
cd C:\Users\Iwan\Desktop\Rusty\Rustyfin
cargo build -p rustfin-server 2>&1 | tail -5
```
Expected: `Compiling rustfin-server ...` then `Finished`

**Step 3: Commit**

```bash
git add crates/server/src/routes.rs
git commit -m "feat: allow music kind in create_library"
```

---

## Task 2: Add `duration_ms` to `ItemRow` and `get_children`

**Files:**
- Modify: `crates/db/src/repo/items.rs`

**Step 1: Add `duration_ms` field to `ItemRow`**

In `items.rs`, the `ItemRow` struct starts at line 3. Add one field at the end:

```rust
#[derive(Debug, Clone)]
pub struct ItemRow {
    pub id: String,
    pub library_id: String,
    pub kind: String,
    pub parent_id: Option<String>,
    pub title: String,
    pub sort_title: Option<String>,
    pub year: Option<i64>,
    pub overview: Option<String>,
    pub poster_url: Option<String>,
    pub backdrop_url: Option<String>,
    pub logo_url: Option<String>,
    pub thumb_url: Option<String>,
    pub created_ts: i64,
    pub updated_ts: i64,
    pub duration_ms: Option<i64>,   // new
}
```

**Step 2: Update `row_to_item` to set `duration_ms: None`**

`row_to_item` at line 220 maps a 14-element tuple to `ItemRow`. It doesn't change its tuple type — just add the new field at the end of the struct literal:

```rust
fn row_to_item(
    r: (
        String,
        String,
        String,
        Option<String>,
        String,
        Option<String>,
        Option<i64>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        i64,
        i64,
    ),
) -> ItemRow {
    ItemRow {
        id: r.0,
        library_id: r.1,
        kind: r.2,
        parent_id: r.3,
        title: r.4,
        sort_title: r.5,
        year: r.6,
        overview: r.7,
        poster_url: r.8,
        backdrop_url: r.9,
        logo_url: r.10,
        thumb_url: r.11,
        created_ts: r.12,
        updated_ts: r.13,
        duration_ms: None,   // new
    }
}
```

**Step 3: Add `row_to_item_full` for 15-element tuples (with duration)**

Add this new function immediately after `row_to_item`:

```rust
fn row_to_item_full(
    r: (
        String,
        String,
        String,
        Option<String>,
        String,
        Option<String>,
        Option<i64>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        i64,
        i64,
        Option<i64>,
    ),
) -> ItemRow {
    ItemRow {
        id: r.0,
        library_id: r.1,
        kind: r.2,
        parent_id: r.3,
        title: r.4,
        sort_title: r.5,
        year: r.6,
        overview: r.7,
        poster_url: r.8,
        backdrop_url: r.9,
        logo_url: r.10,
        thumb_url: r.11,
        created_ts: r.12,
        updated_ts: r.13,
        duration_ms: r.14,
    }
}
```

**Step 4: Update `get_children` to JOIN with `media_file` and use `row_to_item_full`**

Replace the entire `get_children` function (lines 49–75) with:

```rust
pub async fn get_children(pool: &SqlitePool, parent_id: &str) -> Result<Vec<ItemRow>, sqlx::Error> {
    let rows: Vec<(
        String,
        String,
        String,
        Option<String>,
        String,
        Option<String>,
        Option<i64>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        i64,
        i64,
        Option<i64>,
    )> = sqlx::query_as(
        "SELECT i.id, i.library_id, i.kind, i.parent_id, i.title, i.sort_title, i.year, \
         i.overview, i.poster_url, i.backdrop_url, i.logo_url, i.thumb_url, \
         i.created_ts, i.updated_ts, mf.duration_ms \
         FROM item i \
         LEFT JOIN episode_file_map efm ON efm.episode_item_id = i.id \
         LEFT JOIN media_file mf ON mf.id = efm.file_id \
         WHERE i.parent_id = ? ORDER BY i.title",
    )
    .bind(parent_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(row_to_item_full).collect())
}
```

**Step 5: Verify it compiles**

```bash
cargo build -p rustfin-db 2>&1 | tail -5
```
Expected: `Finished`

**Step 6: Commit**

```bash
git add crates/db/src/repo/items.rs
git commit -m "feat: add duration_ms to ItemRow, join media_file in get_children"
```

---

## Task 3: Expose `duration_ms` in `ItemResponse`

**Files:**
- Modify: `crates/server/src/routes.rs`

**Step 1: Add `duration_ms` to `ItemResponse` struct**

The struct is at line 970. Add the field:

```rust
#[derive(Serialize)]
struct ItemResponse {
    id: String,
    library_id: String,
    kind: String,
    parent_id: Option<String>,
    title: String,
    sort_title: Option<String>,
    year: Option<i64>,
    overview: Option<String>,
    poster_url: Option<String>,
    backdrop_url: Option<String>,
    logo_url: Option<String>,
    thumb_url: Option<String>,
    created_ts: i64,
    updated_ts: i64,
    duration_ms: Option<i64>,   // new
}
```

**Step 2: Pass it through in `item_to_response`**

The function is at line 1005. Add the field at the end of the returned struct:

```rust
fn item_to_response(item: rustfin_db::repo::items::ItemRow, include_images: bool) -> ItemResponse {
    ItemResponse {
        id: item.id.clone(),
        library_id: item.library_id,
        kind: item.kind,
        parent_id: item.parent_id,
        title: item.title,
        sort_title: item.sort_title,
        year: item.year,
        overview: item.overview,
        poster_url: if item.poster_url.is_some() {
            item_image_url(&item.id, "poster", include_images)
        } else {
            None
        },
        backdrop_url: if item.backdrop_url.is_some() {
            item_image_url(&item.id, "backdrop", include_images)
        } else {
            None
        },
        logo_url: if item.logo_url.is_some() {
            item_image_url(&item.id, "logo", include_images)
        } else {
            None
        },
        thumb_url: if item.thumb_url.is_some() {
            item_image_url(&item.id, "thumb", include_images)
        } else {
            None
        },
        created_ts: item.created_ts,
        updated_ts: item.updated_ts,
        duration_ms: item.duration_ms,   // new
    }
}
```

**Step 3: Verify it compiles**

```bash
cargo build -p rustfin-server 2>&1 | tail -5
```
Expected: `Finished`

**Step 4: Commit**

```bash
git add crates/server/src/routes.rs
git commit -m "feat: expose duration_ms in ItemResponse"
```

---

## Task 4: Add Music option to admin library kind dropdown

**Files:**
- Modify: `ui/src/app/admin/page.tsx`

**Step 1: Find the kind `<select>` element**

Search for `option value="tv_shows"` in `admin/page.tsx` — it's around line 517. The block is:

```tsx
<option value="movies">Movies</option>
<option value="tv_shows">TV Shows</option>
```

Add Music after TV Shows:

```tsx
<option value="movies">Movies</option>
<option value="tv_shows">TV Shows</option>
<option value="music">Music</option>
```

**Step 2: Verify in browser**

Start the dev server (`npm run dev` in `ui/`). Go to `/admin`, find "Create Library" — the kind dropdown should now show "Movies / TV Shows / Music".

**Step 3: Commit**

```bash
git add ui/src/app/admin/page.tsx
git commit -m "feat: add Music option to library kind dropdown"
```

---

## Task 5: Create `MusicPlayerContext`

**Files:**
- Create: `ui/src/lib/musicPlayerContext.tsx`

**Step 1: Create the file**

```tsx
'use client';

import { createContext, useContext, useRef, useState, useCallback, useEffect, type ReactNode } from 'react';
import { apiJson } from '@/lib/api';

export interface MusicTrack {
  id: string;
  title: string;
  artist: string;
  albumTitle: string;
  albumArtUrl?: string;
  durationMs?: number;
}

interface PlaybackDescriptor {
  direct_url: string;
}

interface MusicPlayerContextValue {
  queue: MusicTrack[];
  currentIndex: number;
  playing: boolean;
  progress: number;   // seconds
  duration: number;   // seconds
  volume: number;
  currentTrack: MusicTrack | null;
  playQueue: (tracks: MusicTrack[], startIndex?: number) => void;
  playPause: () => void;
  seek: (seconds: number) => void;
  next: () => void;
  prev: () => void;
  setVolume: (v: number) => void;
  stop: () => void;
}

const MusicPlayerContext = createContext<MusicPlayerContextValue | null>(null);

export function MusicPlayerProvider({ children }: { children: ReactNode }) {
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const [queue, setQueue] = useState<MusicTrack[]>([]);
  const [currentIndex, setCurrentIndex] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [progress, setProgress] = useState(0);
  const [duration, setDuration] = useState(0);
  const [volume, setVolumeState] = useState(1);

  // Create hidden audio element once
  useEffect(() => {
    const audio = new Audio();
    audio.volume = 1;
    audioRef.current = audio;

    audio.addEventListener('timeupdate', () => setProgress(audio.currentTime));
    audio.addEventListener('durationchange', () => setDuration(audio.duration || 0));
    audio.addEventListener('ended', () => {
      setCurrentIndex((idx) => idx + 1);
    });
    audio.addEventListener('play', () => setPlaying(true));
    audio.addEventListener('pause', () => setPlaying(false));

    return () => {
      audio.pause();
      audio.src = '';
    };
  }, []);

  // Load and play when currentIndex or queue changes
  useEffect(() => {
    if (queue.length === 0) return;
    if (currentIndex >= queue.length) {
      // Reached end of queue
      setPlaying(false);
      setCurrentIndex(0);
      return;
    }
    const track = queue[currentIndex];
    const audio = audioRef.current;
    if (!audio) return;

    apiJson<PlaybackDescriptor>(`/items/${track.id}/playback`)
      .then((desc) => {
        audio.src = desc.direct_url;
        audio.play().catch(() => {});
      })
      .catch(() => {});
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [currentIndex, queue]);

  const playQueue = useCallback((tracks: MusicTrack[], startIndex = 0) => {
    setQueue(tracks);
    setCurrentIndex(startIndex);
    setProgress(0);
    setDuration(0);
  }, []);

  const playPause = useCallback(() => {
    const audio = audioRef.current;
    if (!audio) return;
    if (audio.paused) {
      audio.play().catch(() => {});
    } else {
      audio.pause();
    }
  }, []);

  const seek = useCallback((seconds: number) => {
    const audio = audioRef.current;
    if (audio) {
      audio.currentTime = seconds;
      setProgress(seconds);
    }
  }, []);

  const next = useCallback(() => {
    setCurrentIndex((idx) => Math.min(idx + 1, queue.length - 1));
    setProgress(0);
  }, [queue.length]);

  const prev = useCallback(() => {
    const audio = audioRef.current;
    // If more than 3 seconds in, restart current track; otherwise go back
    if (audio && audio.currentTime > 3) {
      audio.currentTime = 0;
      setProgress(0);
    } else {
      setCurrentIndex((idx) => Math.max(idx - 1, 0));
      setProgress(0);
    }
  }, []);

  const setVolume = useCallback((v: number) => {
    const audio = audioRef.current;
    if (audio) audio.volume = v;
    setVolumeState(v);
  }, []);

  const stop = useCallback(() => {
    const audio = audioRef.current;
    if (audio) {
      audio.pause();
      audio.src = '';
    }
    setQueue([]);
    setCurrentIndex(0);
    setProgress(0);
    setDuration(0);
    setPlaying(false);
  }, []);

  const currentTrack = queue.length > 0 && currentIndex < queue.length
    ? queue[currentIndex]
    : null;

  return (
    <MusicPlayerContext.Provider
      value={{
        queue,
        currentIndex,
        playing,
        progress,
        duration,
        volume,
        currentTrack,
        playQueue,
        playPause,
        seek,
        next,
        prev,
        setVolume,
        stop,
      }}
    >
      {children}
    </MusicPlayerContext.Provider>
  );
}

export function useMusicPlayer() {
  const ctx = useContext(MusicPlayerContext);
  if (!ctx) throw new Error('useMusicPlayer must be used inside MusicPlayerProvider');
  return ctx;
}
```

**Step 2: Verify TypeScript compiles**

```bash
cd ui && npx tsc --noEmit 2>&1 | head -20
```
Expected: no errors for this file (warnings about other files are fine)

**Step 3: Commit**

```bash
git add ui/src/lib/musicPlayerContext.tsx
git commit -m "feat: add MusicPlayerContext with hidden audio element"
```

---

## Task 6: Create `MiniPlayer` component

**Files:**
- Create: `ui/src/app/components/MiniPlayer.tsx`

Note: create the `components/` directory inside `ui/src/app/` if it doesn't exist.

**Step 1: Create the file**

```tsx
'use client';

import { useMusicPlayer } from '@/lib/musicPlayerContext';

function formatSeconds(s: number): string {
  if (!isFinite(s) || s < 0) return '0:00';
  const m = Math.floor(s / 60);
  const sec = Math.floor(s % 60).toString().padStart(2, '0');
  return `${m}:${sec}`;
}

export default function MiniPlayer() {
  const {
    queue,
    currentTrack,
    playing,
    progress,
    duration,
    volume,
    currentIndex,
    playPause,
    seek,
    next,
    prev,
    setVolume,
    stop,
  } = useMusicPlayer();

  if (queue.length === 0 || !currentTrack) return null;

  return (
    <div className="fixed bottom-0 left-0 right-0 z-40 border-t border-[var(--border)] bg-[var(--surface)] px-4 py-2 flex items-center gap-4">
      {/* Album art + track info */}
      <div className="flex items-center gap-3 min-w-0 w-56 shrink-0">
        {currentTrack.albumArtUrl ? (
          <img
            src={currentTrack.albumArtUrl}
            alt={currentTrack.albumTitle}
            className="w-10 h-10 rounded object-cover shrink-0"
          />
        ) : (
          <div className="w-10 h-10 rounded bg-white/10 flex items-center justify-center shrink-0 text-lg">
            ♪
          </div>
        )}
        <div className="min-w-0">
          <p className="text-sm font-medium truncate">{currentTrack.title}</p>
          <p className="text-xs muted truncate">{currentTrack.artist}</p>
        </div>
      </div>

      {/* Controls + seek */}
      <div className="flex-1 flex flex-col items-center gap-1 min-w-0">
        <div className="flex items-center gap-3">
          <button
            onClick={prev}
            disabled={currentIndex === 0}
            className="btn-ghost px-2 py-1 text-base disabled:opacity-30"
            title="Previous"
          >
            ⏮
          </button>
          <button
            onClick={playPause}
            className="btn-primary px-3 py-1.5 text-sm rounded-full"
            title={playing ? 'Pause' : 'Play'}
          >
            {playing ? '⏸' : '▶'}
          </button>
          <button
            onClick={next}
            disabled={currentIndex >= queue.length - 1}
            className="btn-ghost px-2 py-1 text-base disabled:opacity-30"
            title="Next"
          >
            ⏭
          </button>
        </div>
        <div className="flex items-center gap-2 w-full max-w-md">
          <span className="text-xs muted w-8 text-right shrink-0">{formatSeconds(progress)}</span>
          <input
            type="range"
            min={0}
            max={duration || 1}
            step={0.1}
            value={progress}
            onChange={(e) => seek(Number(e.target.value))}
            className="flex-1 h-1 accent-[var(--orange-soft)]"
          />
          <span className="text-xs muted w-8 shrink-0">{formatSeconds(duration)}</span>
        </div>
      </div>

      {/* Volume + stop */}
      <div className="flex items-center gap-2 w-32 shrink-0 justify-end">
        <span className="text-sm muted">🔊</span>
        <input
          type="range"
          min={0}
          max={1}
          step={0.01}
          value={volume}
          onChange={(e) => setVolume(Number(e.target.value))}
          className="w-16 h-1 accent-[var(--orange-soft)]"
        />
        <button
          onClick={stop}
          className="btn-ghost px-2 py-1 text-xs muted"
          title="Stop"
        >
          ✕
        </button>
      </div>
    </div>
  );
}
```

**Step 2: Verify TypeScript compiles**

```bash
cd ui && npx tsc --noEmit 2>&1 | head -20
```
Expected: no errors

**Step 3: Commit**

```bash
git add ui/src/app/components/MiniPlayer.tsx
git commit -m "feat: add MiniPlayer fixed bottom bar component"
```

---

## Task 7: Wire `MusicPlayerProvider` and `MiniPlayer` into the app

**Files:**
- Modify: `ui/src/app/providers.tsx`
- Modify: `ui/src/app/layout.tsx`

**Step 1: Add `MusicPlayerProvider` to the providers tree**

`providers.tsx` currently wraps children in `AuthProvider` > `ChannelsProvider`. Add `MusicPlayerProvider` as the innermost wrapper:

```tsx
'use client';

import { AuthProvider } from '@/lib/auth';
import { ChannelsProvider } from '@/lib/channelsContext';
import { MusicPlayerProvider } from '@/lib/musicPlayerContext';

export default function Providers({ children }: { children: React.ReactNode }) {
  return (
    <AuthProvider>
      <ChannelsProvider>
        <MusicPlayerProvider>{children}</MusicPlayerProvider>
      </ChannelsProvider>
    </AuthProvider>
  );
}
```

**Step 2: Add `MiniPlayer` to the root layout**

`layout.tsx` renders `<Providers>` wrapping the page content. Import `MiniPlayer` and render it inside `<Providers>` (so it has access to the context), after `<main>` and before `<footer>`. Also add bottom padding to the page so content isn't hidden behind the player bar.

```tsx
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
```

Note: `pb-8` changed to `pb-24` to give room for the MiniPlayer bar (about 64px tall).

**Step 3: Verify in browser**

Visit any page. No MiniPlayer should appear yet (queue is empty). Check browser console for errors.

**Step 4: Commit**

```bash
git add ui/src/app/providers.tsx ui/src/app/layout.tsx
git commit -m "feat: mount MusicPlayerProvider and MiniPlayer in root layout"
```

---

## Task 8: Update library browser for music kind

**Files:**
- Modify: `ui/src/app/libraries/[id]/page.tsx`

**Step 1: Rewrite the page to handle music kind**

Replace the entire file content:

```tsx
'use client';

import { useEffect, useState } from 'react';
import Link from 'next/link';
import { useParams } from 'next/navigation';
import { apiJson } from '@/lib/api';

interface Library {
  id: string;
  name: string;
  kind: string;
}

interface Item {
  id: string;
  title: string;
  kind: string;
  year?: number;
  poster_url?: string;
}

export default function LibraryPage() {
  const params = useParams();
  const id = params.id as string;
  const [library, setLibrary] = useState<Library | null>(null);
  const [items, setItems] = useState<Item[]>([]);
  const [query, setQuery] = useState('');
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    Promise.all([
      apiJson<Library>(`/libraries/${id}`),
      apiJson<Item[]>(`/libraries/${id}/items`),
    ])
      .then(([lib, libItems]) => {
        setLibrary(lib);
        setItems(libItems);
      })
      .catch(() => {})
      .finally(() => setLoading(false));
  }, [id]);

  const q = query.trim().toLowerCase();
  const visibleItems = q
    ? items.filter((item) => item.title.toLowerCase().includes(q))
    : items;

  if (loading) {
    return (
      <div className="panel-soft animate-rise px-5 py-4">
        <p className="text-sm muted">Loading library...</p>
      </div>
    );
  }

  const isMusic = library?.kind === 'music';

  return (
    <div className="space-y-6 animate-rise">
      <header className="space-y-2">
        <span className="chip">{isMusic ? 'Music Library' : 'Library View'}</span>
        <h1 className="text-3xl font-semibold">{library?.name ?? 'Library'}</h1>
        <p className="text-sm muted">
          Showing {visibleItems.length} of {items.length} {isMusic ? 'artists' : 'items'}
        </p>
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder={isMusic ? 'Search artists…' : 'Search titles in this library'}
          className="input mt-2 w-full max-w-md px-3 py-2 text-sm"
        />
      </header>

      {visibleItems.length === 0 ? (
        <div className="panel px-6 py-8">
          <p className="text-sm muted">
            {items.length === 0
              ? 'No media items were found in this library yet. Try scanning the library.'
              : 'No items match your search.'}
          </p>
        </div>
      ) : isMusic ? (
        /* ── Music: square artist grid ── */
        <div className="grid grid-cols-2 gap-4 md:grid-cols-4 lg:grid-cols-6">
          {visibleItems.map((item) => (
            <Link key={item.id} href={`/items/${item.id}`} className="group block">
              <div className="tile tile-hover aspect-square overflow-hidden">
                {item.poster_url ? (
                  <img
                    src={item.poster_url}
                    alt={item.title}
                    className="h-full w-full object-cover transition duration-300 group-hover:scale-105"
                  />
                ) : (
                  <div className="flex h-full w-full items-center justify-center text-4xl muted">
                    ♪
                  </div>
                )}
              </div>
              <p className="mt-2 truncate text-sm font-medium">{item.title}</p>
              <p className="text-xs muted">Artist</p>
            </Link>
          ))}
        </div>
      ) : (
        /* ── Video: existing poster grid ── */
        <div className="grid grid-cols-2 gap-4 md:grid-cols-4 lg:grid-cols-6">
          {visibleItems.map((item) => (
            <Link key={item.id} href={`/items/${item.id}`} className="group block">
              <div className="tile tile-hover aspect-[2/3] overflow-hidden">
                {item.poster_url ? (
                  <img
                    src={item.poster_url}
                    alt={item.title}
                    className="h-full w-full object-cover transition duration-300 group-hover:scale-105"
                  />
                ) : (
                  <div className="flex h-full w-full items-center justify-center text-xs muted">
                    No Poster
                  </div>
                )}
              </div>
              <p className="mt-2 truncate text-sm font-medium">{item.title}</p>
              {item.year && <p className="text-xs muted">{item.year}</p>}
            </Link>
          ))}
        </div>
      )}
    </div>
  );
}
```

**Step 2: Verify in browser**

Navigate to a music library. You should see a square art grid with artist names.
Navigate to a movie library — it should look exactly as before.

**Step 3: Commit**

```bash
git add "ui/src/app/libraries/[id]/page.tsx"
git commit -m "feat: music-aware library browser with square artist grid"
```

---

## Task 9: Update item page for artist, album, and track kinds

**Files:**
- Modify: `ui/src/app/items/[id]/page.tsx`

**Step 1: Rewrite the item page**

Replace the entire file content:

```tsx
'use client';

import { useEffect, useState } from 'react';
import Link from 'next/link';
import { useParams } from 'next/navigation';
import { apiJson } from '@/lib/api';
import { useMusicPlayer, type MusicTrack } from '@/lib/musicPlayerContext';

interface Item {
  id: string;
  title: string;
  kind: string;
  year?: number;
  overview?: string;
  poster_url?: string;
  backdrop_url?: string;
  thumb_url?: string;
  parent_id?: string;
  library_id: string;
}

interface Child {
  id: string;
  title: string;
  kind: string;
  year?: number;
  poster_url?: string;
  thumb_url?: string;
  duration_ms?: number;
}

function formatDuration(ms?: number): string {
  if (!ms) return '';
  const total = Math.floor(ms / 1000);
  const m = Math.floor(total / 60);
  const s = (total % 60).toString().padStart(2, '0');
  return `${m}:${s}`;
}

export default function ItemPage() {
  const params = useParams();
  const id = params.id as string;
  const [item, setItem] = useState<Item | null>(null);
  const [children, setChildren] = useState<Child[]>([]);
  const [parentItem, setParentItem] = useState<Item | null>(null);
  const { playQueue, currentTrack, playing, playPause } = useMusicPlayer();

  useEffect(() => {
    setItem(null);
    setChildren([]);
    setParentItem(null);
    apiJson<Item>(`/items/${id}`).then(setItem).catch(() => {});
    apiJson<Child[]>(`/items/${id}/children`).then(setChildren).catch(() => {});
  }, [id]);

  // Fetch parent for album pages (to get artist name)
  useEffect(() => {
    if (item?.kind === 'album' && item.parent_id) {
      apiJson<Item>(`/items/${item.parent_id}`).then(setParentItem).catch(() => {});
    }
  }, [item]);

  if (!item) {
    return (
      <div className="panel-soft animate-rise px-5 py-4">
        <p className="text-sm muted">Loading...</p>
      </div>
    );
  }

  // ── Artist page ────────────────────────────────────────────────────────────
  if (item.kind === 'artist') {
    return (
      <div className="space-y-7 animate-rise">
        <header className="space-y-2">
          <span className="chip">Artist</span>
          <h1 className="text-3xl font-semibold">{item.title}</h1>
          {item.overview && <p className="max-w-3xl leading-relaxed muted">{item.overview}</p>}
        </header>

        {children.length > 0 && (
          <section className="space-y-4">
            <h2 className="text-xl font-semibold">Albums</h2>
            <div className="grid grid-cols-2 gap-4 md:grid-cols-4 lg:grid-cols-6">
              {children.map((album) => (
                <Link key={album.id} href={`/items/${album.id}`} className="group block">
                  <div className="tile tile-hover aspect-square overflow-hidden">
                    {album.poster_url ? (
                      <img
                        src={album.poster_url}
                        alt={album.title}
                        className="h-full w-full object-cover transition duration-300 group-hover:scale-105"
                      />
                    ) : (
                      <div className="flex h-full w-full items-center justify-center text-4xl muted">
                        ♪
                      </div>
                    )}
                  </div>
                  <p className="mt-2 truncate text-sm font-medium">{album.title}</p>
                  {album.year && <p className="text-xs muted">{album.year}</p>}
                </Link>
              ))}
            </div>
          </section>
        )}
      </div>
    );
  }

  // ── Album page ─────────────────────────────────────────────────────────────
  if (item.kind === 'album') {
    const artistName = parentItem?.title ?? '';

    function buildQueue(tracks: Child[], albumArtUrl?: string): MusicTrack[] {
      return tracks.map((t) => ({
        id: t.id,
        title: t.title,
        artist: artistName,
        albumTitle: item!.title,
        albumArtUrl,
        durationMs: t.duration_ms,
      }));
    }

    const albumArtUrl = item.poster_url ?? undefined;

    return (
      <div className="space-y-7 animate-rise">
        <div className="flex flex-col gap-6 sm:flex-row">
          {/* Album art */}
          <div className="w-48 shrink-0">
            <div className="tile overflow-hidden aspect-square">
              {albumArtUrl ? (
                <img src={albumArtUrl} alt={item.title} className="w-full h-full object-cover" />
              ) : (
                <div className="flex h-full w-full items-center justify-center text-5xl muted">
                  ♪
                </div>
              )}
            </div>
          </div>

          {/* Album meta */}
          <div className="flex-1 space-y-3">
            <div className="space-y-1">
              <span className="chip">Album</span>
              <h1 className="text-3xl font-semibold">{item.title}</h1>
              {artistName && (
                <Link href={`/items/${item.parent_id}`} className="text-sm muted hover:underline">
                  {artistName}
                </Link>
              )}
              {item.year && <p className="text-sm muted">{item.year}</p>}
            </div>

            {children.length > 0 && (
              <button
                onClick={() => playQueue(buildQueue(children, albumArtUrl), 0)}
                className="btn-primary px-5 py-2 text-sm"
              >
                ▶ Play Album
              </button>
            )}
          </div>
        </div>

        {/* Track list */}
        {children.length > 0 && (
          <section className="space-y-1">
            {children.map((track, idx) => {
              const isPlaying = currentTrack?.id === track.id && playing;
              const isCurrent = currentTrack?.id === track.id;
              return (
                <div
                  key={track.id}
                  className={[
                    'flex items-center gap-3 px-3 py-2 rounded-lg group cursor-pointer select-none',
                    isCurrent ? 'bg-white/10' : 'hover:bg-white/5',
                  ].join(' ')}
                  onClick={() => {
                    if (isCurrent) {
                      playPause();
                    } else {
                      playQueue(buildQueue(children, albumArtUrl), idx);
                    }
                  }}
                >
                  <span className="w-6 text-center text-xs muted shrink-0">
                    {isPlaying ? '▶' : (isCurrent ? '❙❙' : idx + 1)}
                  </span>
                  <span className={['flex-1 text-sm truncate', isCurrent ? 'text-[var(--orange-soft)]' : ''].join(' ')}>
                    {track.title}
                  </span>
                  <span className="text-xs muted shrink-0">{formatDuration(track.duration_ms)}</span>
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      if (isCurrent) {
                        playPause();
                      } else {
                        playQueue(buildQueue(children, albumArtUrl), idx);
                      }
                    }}
                    className="btn-ghost px-2 py-0.5 text-xs opacity-0 group-hover:opacity-100"
                  >
                    {isPlaying ? '⏸' : '▶'}
                  </button>
                </div>
              );
            })}
          </section>
        )}
      </div>
    );
  }

  // ── Track page (direct navigation) ────────────────────────────────────────
  if (item.kind === 'track') {
    return (
      <div className="space-y-6 animate-rise">
        <span className="chip">Track</span>
        <h1 className="text-3xl font-semibold">{item.title}</h1>
        {item.parent_id && (
          <Link href={`/items/${item.parent_id}`} className="text-sm muted hover:underline block">
            ← Back to album
          </Link>
        )}
        <button
          onClick={() =>
            playQueue([
              {
                id: item.id,
                title: item.title,
                artist: '',
                albumTitle: '',
                albumArtUrl: item.poster_url ?? undefined,
              },
            ])
          }
          className="btn-primary px-6 py-2.5 text-sm"
        >
          ▶ Play
        </button>
      </div>
    );
  }

  // ── Default: movies / series / episodes (existing behaviour) ──────────────
  const isPlayable = item.kind === 'movie' || item.kind === 'episode';

  return (
    <div className="space-y-7 animate-rise">
      {item.backdrop_url && (
        <div className="tile relative h-64 overflow-hidden rounded-2xl sm:h-72">
          <img
            src={item.backdrop_url}
            alt=""
            className="h-full w-full object-cover opacity-45"
          />
          <div className="absolute inset-0 bg-gradient-to-t from-[#1f2535] via-[#1f2535]/50 to-transparent" />
        </div>
      )}

      <div className="flex flex-col gap-6 lg:flex-row">
        {item.poster_url && (
          <div className="w-52 flex-shrink-0">
            <div className="tile overflow-hidden">
              <img src={item.poster_url} alt={item.title} className="w-full" />
            </div>
          </div>
        )}
        <div className="flex-1 space-y-4">
          <div className="space-y-2">
            <span className="chip chip-accent">{item.kind.toUpperCase()}</span>
            <h1 className="text-3xl font-semibold sm:text-4xl">{item.title}</h1>
            {item.year && <span className="text-sm muted">{item.year}</span>}
          </div>

          {item.overview && <p className="max-w-3xl leading-relaxed muted">{item.overview}</p>}

          {isPlayable && (
            <Link
              href={`/player/${id}`}
              className="btn-primary inline-flex px-6 py-2.5 text-sm"
            >
              Play Now
            </Link>
          )}
        </div>
      </div>

      {children.length > 0 && (
        <section className="space-y-4">
          <h2 className="text-xl font-semibold">
            {item.kind === 'series' ? 'Seasons' : 'Episodes'}
          </h2>
          <div className="grid grid-cols-2 gap-3 md:grid-cols-4 lg:grid-cols-6">
            {children.map((child) => (
              <Link
                key={child.id}
                href={`/items/${child.id}`}
                className="tile tile-hover block overflow-hidden"
              >
                {child.poster_url || child.thumb_url ? (
                  <img
                    src={child.poster_url || child.thumb_url}
                    alt={child.title}
                    className="aspect-[2/3] w-full object-cover"
                  />
                ) : (
                  <div className="flex aspect-[2/3] items-center justify-center bg-white/5 px-2 text-xs muted">
                    {child.kind.toUpperCase()}
                  </div>
                )}
                <div className="space-y-1 p-3">
                  <p className="font-medium text-sm">{child.title}</p>
                  {child.kind === 'episode' && (
                    <p className="text-xs muted">Episode</p>
                  )}
                </div>
              </Link>
            ))}
          </div>
        </section>
      )}
    </div>
  );
}
```

**Step 2: Verify TypeScript compiles**

```bash
cd ui && npx tsc --noEmit 2>&1 | head -30
```
Expected: no errors

**Step 3: End-to-end browser test**

1. Go to Admin → Create Library, kind = Music, point to a folder with mp3 files
2. Click Scan
3. Navigate to the library — you should see artist tiles
4. Click an artist — you should see album tiles
5. Click an album — you should see track list with durations
6. Click a track row or ▶ — MiniPlayer appears at the bottom
7. When a track ends the next one starts automatically
8. ⏮/⏭ skip correctly
9. ✕ in MiniPlayer dismisses it

**Step 4: Commit**

```bash
git add "ui/src/app/items/[id]/page.tsx"
git commit -m "feat: music-aware item page — artist/album/track views with mini-player"
```

---

## Done

All 9 tasks complete. The feature is fully implemented:
- Admins can create music libraries via the existing admin panel
- The scanner discovers mp3, flac, aac, m4a, ogg, opus, wav, wma, aiff, alac
- Library browser shows artist tiles; drilldown shows albums then track lists
- A persistent mini-player bar plays audio across page navigations with prev/next/seek/volume
