'use client';

import { useEffect, useMemo, useState } from 'react';
import Link from 'next/link';
import { useRouter } from 'next/navigation';
import Image from 'next/image';
import { useAuth } from '@/lib/auth';
import { apiJson } from '@/lib/api';
import { listCalendarEvents, type CalendarEvent } from '@/lib/calendarApi';
import { findDataDeleteTarget, playTelegramDeleteAnimation } from '@/lib/deleteAnimation';
import { clientErrorMessage } from '@/lib/errors';
import { getPublicSystemInfo } from '@/lib/setupApi';
import { listPublicRooms, type PublicRoom } from '@/lib/watchPartyApi';

interface ContinueWatchingItem {
  id: string;
  library_id: string;
  title: string;
  kind: string;
  year?: number;
  poster_url?: string;
  progress_ms: number;
  duration_ms?: number | null;
  last_played_ts: number;
}

function normalizeContinueWatchingItems(raw: unknown): ContinueWatchingItem[] {
  if (!Array.isArray(raw)) return [];
  return raw.flatMap((entry) => {
    if (typeof entry !== 'object' || entry === null || Array.isArray(entry)) {
      return [];
    }
    const item = entry as Partial<ContinueWatchingItem>;
    if (
      typeof item.id !== 'string' ||
      typeof item.library_id !== 'string' ||
      typeof item.title !== 'string' ||
      typeof item.kind !== 'string' ||
      typeof item.progress_ms !== 'number' ||
      typeof item.last_played_ts !== 'number'
    ) {
      return [];
    }
    return [{
      id: item.id,
      library_id: item.library_id,
      title: item.title,
      kind: item.kind,
      year: typeof item.year === 'number' ? item.year : undefined,
      poster_url: typeof item.poster_url === 'string' ? item.poster_url : undefined,
      progress_ms: item.progress_ms,
      duration_ms:
        typeof item.duration_ms === 'number' || item.duration_ms === null
          ? item.duration_ms
          : undefined,
      last_played_ts: item.last_played_ts,
    }];
  });
}

function normalizePublicRooms(raw: unknown): PublicRoom[] {
  if (!Array.isArray(raw)) return [];
  return raw.flatMap((entry) => {
    if (typeof entry !== 'object' || entry === null || Array.isArray(entry)) {
      return [];
    }
    const room = entry as Partial<PublicRoom>;
    if (
      typeof room.room_id !== 'string' ||
      typeof room.host_username !== 'string' ||
      typeof room.title !== 'string' ||
      typeof room.room_mode !== 'string' ||
      typeof room.password_required !== 'boolean' ||
      typeof room.member_count !== 'number' ||
      typeof room.created_ts !== 'number'
    ) {
      return [];
    }
    return [room as PublicRoom];
  });
}

function normalizeCalendarEvents(raw: unknown): CalendarEvent[] {
  if (!Array.isArray(raw)) return [];
  return raw.flatMap((entry) => {
    if (typeof entry !== 'object' || entry === null || Array.isArray(entry)) {
      return [];
    }
    const event = entry as Partial<CalendarEvent>;
    if (
      typeof event.id !== 'string' ||
      typeof event.occurrence_id !== 'string' ||
      typeof event.title !== 'string' ||
      typeof event.event_date !== 'string'
    ) {
      return [];
    }
    return [event as CalendarEvent];
  });
}

function formatDurationLabel(totalSeconds: number): string {
  const safeTotal = Math.max(0, Math.floor(totalSeconds));
  const hours = Math.floor(safeTotal / 3600);
  const minutes = Math.floor((safeTotal % 3600) / 60);
  const seconds = safeTotal % 60;
  if (hours > 0) {
    return `${hours}:${String(minutes).padStart(2, '0')}:${String(seconds).padStart(2, '0')}`;
  }
  return `${minutes}:${String(seconds).padStart(2, '0')}`;
}

function formatRoomMembersLabel(memberCount: number): string {
  return memberCount === 1 ? '1 member' : `${memberCount} members`;
}

function withNoon(date: Date): Date {
  const next = new Date(date);
  next.setHours(12, 0, 0, 0);
  return next;
}

function addDays(date: Date, days: number): Date {
  const next = withNoon(date);
  next.setDate(next.getDate() + days);
  return next;
}

function formatYmd(date: Date): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, '0');
  const day = String(date.getDate()).padStart(2, '0');
  return `${year}-${month}-${day}`;
}

export default function HomePage() {
  const router = useRouter();
  const { me, loading: authLoading } = useAuth();

  const [setupChecked, setSetupChecked] = useState(false);
  const [setupComplete, setSetupComplete] = useState(true);
  const [loadingData, setLoadingData] = useState(false);
  const [continueWatching, setContinueWatching] = useState<ContinueWatchingItem[]>([]);
  const [dismissingContinueItemIds, setDismissingContinueItemIds] = useState<string[]>([]);
  const [publicRooms, setPublicRooms] = useState<PublicRoom[]>([]);
  const [upcomingCalendarEvents, setUpcomingCalendarEvents] = useState<CalendarEvent[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    getPublicSystemInfo()
      .then((info) => {
        if (cancelled) return;
        setSetupComplete(info.setup_completed);
        setSetupChecked(true);
        if (!info.setup_completed) {
          router.replace('/setup');
        }
      })
      .catch(() => {
        if (!cancelled) {
          setSetupChecked(true);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [router]);

  useEffect(() => {
    if (setupChecked && setupComplete && !authLoading && !me) {
      router.replace('/login');
    }
  }, [authLoading, me, router, setupChecked, setupComplete]);

  useEffect(() => {
    let cancelled = false;
    if (!setupComplete || authLoading || !me) {
      return () => {
        cancelled = true;
      };
    }

    setLoadingData(true);
    setError(null);

    (async () => {
      try {
        const today = withNoon(new Date());
        const fromYmd = formatYmd(today);
        const toYmd = formatYmd(addDays(today, 6));
        const [continueItems, rooms, calendarEvents] = await Promise.all([
          apiJson<unknown>('/playback/continue').catch(() => []),
          listPublicRooms().catch(() => []),
          listCalendarEvents({ from: fromYmd, to: toYmd, scope: 'all' }).catch(() => []),
        ]);
        if (cancelled) return;
        setContinueWatching(normalizeContinueWatchingItems(continueItems));
        setPublicRooms(normalizePublicRooms(rooms));
        setUpcomingCalendarEvents(normalizeCalendarEvents(calendarEvents));
      } catch (err: unknown) {
        if (!cancelled) {
          setError(clientErrorMessage(err, 'Failed to load home view'));
        }
      } finally {
        if (!cancelled) {
          setLoadingData(false);
        }
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [setupComplete, authLoading, me]);

  async function handleDismissContinueItem(itemId: string) {
    if (dismissingContinueItemIds.includes(itemId)) return;
    setDismissingContinueItemIds((prev) => [...prev, itemId]);
    setError(null);
    try {
      await apiJson<{ ok: boolean }>('/playback/progress', {
        method: 'POST',
        body: JSON.stringify({
          item_id: itemId,
          progress_ms: 0,
          played: false,
        }),
      });
      const target = findDataDeleteTarget('data-home-continue-id', itemId);
      await playTelegramDeleteAnimation(target, 540, { keepHiddenAtEnd: true, collapse: true });
      setContinueWatching((prev) => prev.filter((item) => item.id !== itemId));
    } catch (err: unknown) {
      setError(clientErrorMessage(err, 'Failed to remove from Continue Watching'));
    } finally {
      setDismissingContinueItemIds((prev) => prev.filter((id) => id !== itemId));
    }
  }

  const nextSevenDayCalendar = useMemo(() => {
    const eventsByDate = new Map<string, CalendarEvent[]>();
    for (const event of upcomingCalendarEvents) {
      const current = eventsByDate.get(event.event_date) ?? [];
      current.push(event);
      eventsByDate.set(event.event_date, current);
    }
    for (const [dateKey, rows] of eventsByDate.entries()) {
      rows.sort((left, right) => left.title.localeCompare(right.title, undefined, { sensitivity: 'base' }));
      eventsByDate.set(dateKey, rows);
    }

    const start = withNoon(new Date());
    return Array.from({ length: 7 }, (_, index) => {
      const date = addDays(start, index);
      const dateKey = formatYmd(date);
      const events = eventsByDate.get(dateKey) ?? [];
      return {
        dateKey,
        dateLabel: date.toLocaleDateString(undefined, {
          weekday: 'short',
          month: 'short',
          day: 'numeric',
        }),
        events,
      };
    });
  }, [upcomingCalendarEvents]);

  const hasUpcomingCalendarEvents = nextSevenDayCalendar.some((day) => day.events.length > 0);

  if (!setupChecked) {
    return (
      <div className="rf-flat-empty animate-rise">
        <p className="text-sm muted">Checking setup status...</p>
      </div>
    );
  }

  if (!setupComplete) {
    return (
      <div className="rf-flat-empty animate-rise">
        <p className="text-sm muted">Setup is required. Redirecting to setup wizard...</p>
      </div>
    );
  }

  if (authLoading) {
    return (
      <div className="rf-flat-empty animate-rise">
        <p className="text-sm muted">Loading your home view...</p>
      </div>
    );
  }

  if (!me) {
    return (
      <div className="rf-flat-empty animate-rise">
        <p className="text-sm muted">Redirecting to login...</p>
      </div>
    );
  }

  return (
    <div className="animate-rise rf-flat-page">
      <header className="rf-flat-header">
        <h1 className="text-2xl font-semibold sm:text-3xl">Welcome back, {me.username}</h1>
      </header>

      {error && (
        <div className="notice-error rounded-xl px-4 py-2 text-sm">
          {error}
        </div>
      )}

      <div className="grid grid-cols-1 gap-10 xl:grid-cols-2 xl:items-start">
        <section id="continue-watching" className="rf-flat-section">
          <div className="flex items-end justify-between gap-3">
            <div className="space-y-1">
              <h2 className="text-xl font-semibold sm:text-2xl">Continue Watching</h2>
              <p className="text-xs muted sm:text-sm">Jump back into your most recent titles.</p>
            </div>
            <Link
              href="/libraries#continue-watching"
              className="rf-text-action text-sm"
            >
              View all
            </Link>
          </div>
          {continueWatching.length === 0 ? (
            <div className="space-y-3 text-sm text-white/88">
              <p>Start a movie or episode from a library and Rustyfin will keep your place here.</p>
              <Link href="/libraries" className="rf-text-action mt-3 text-sm">
                Open libraries
              </Link>
            </div>
          ) : (
            <div className="rf-flat-list">
              {continueWatching.slice(0, 3).map((item) => {
                const totalMs = item.duration_ms && item.duration_ms > 0 ? item.duration_ms : null;
                const progressPct = totalMs
                  ? Math.max(0, Math.min(100, (item.progress_ms / totalMs) * 100))
                  : 0;
                const progressLabel = totalMs
                  ? `${formatDurationLabel(item.progress_ms / 1000)} / ${formatDurationLabel(totalMs / 1000)}`
                  : `Resume at ${formatDurationLabel(item.progress_ms / 1000)}`;
                const dismissing = dismissingContinueItemIds.includes(item.id);

                return (
                  <div
                    key={`home-continue-${item.id}`}
                    className="relative"
                    data-home-continue-id={item.id}
                  >
                    <button
                      type="button"
                      className="btn-ghost absolute right-0 top-4 z-20 h-7 w-7 rounded-full p-0 text-white/80 hover:text-white disabled:cursor-not-allowed disabled:opacity-45"
                      onClick={() => void handleDismissContinueItem(item.id)}
                      disabled={dismissing}
                      aria-label={`Remove ${item.title} from Continue Watching`}
                      title="Remove from Continue Watching"
                    >
                      {dismissing ? '…' : '×'}
                    </button>
                    <Link
                      href={`/player/${item.id}`}
                      className="rf-flat-row block pr-10"
                    >
                      <div className="flex min-h-[9rem] gap-4">
                        <div className="h-32 w-24 flex-shrink-0 overflow-hidden rounded-2xl border border-[var(--border)] bg-[var(--panel)]/65">
                          {item.poster_url ? (
                            <Image
                              src={item.poster_url}
                              alt={item.title}
                              width={192}
                              height={288}
                              unoptimized
                              className="h-full w-full object-cover"
                            />
                          ) : (
                            <div className="flex h-full items-center justify-center px-2 text-center text-xs muted">
                              {item.kind.toUpperCase()}
                            </div>
                          )}
                        </div>
                        <div className="flex min-w-0 flex-1 flex-col justify-between">
                          <div className="space-y-2">
                            <div className="flex items-start gap-3">
                              <div className="min-w-0">
                                <p className="truncate text-base font-semibold">{item.title}</p>
                                <p className="text-xs uppercase tracking-[0.24em] text-white/60">
                                  {item.kind === 'episode' ? 'Episode' : 'Movie'}
                                  {item.year ? ` · ${item.year}` : ''}
                                </p>
                              </div>
                            </div>
                            <div className="space-y-2">
                              <div className="rf-progress-track">
                                <div
                                  className="rf-progress-fill"
                                  style={{ width: `${progressPct || 0}%` }}
                                />
                              </div>
                              <p className="text-xs muted">{progressLabel}</p>
                            </div>
                          </div>
                        </div>
                      </div>
                    </Link>
                  </div>
                );
              })}
            </div>
          )}
        </section>

        <section className="rf-flat-section">
          <div className="flex items-end justify-between gap-3">
            <div className="space-y-1">
              <h2 className="text-xl font-semibold sm:text-2xl">Open Rooms</h2>
              <p className="text-xs muted sm:text-sm">See who is active and jump in now.</p>
            </div>
            <Link href="/rooms" className="rf-text-action text-sm">
              View all
            </Link>
          </div>
          {loadingData ? (
            <div className="rf-flat-empty text-sm muted" aria-live="polite">
              Loading open rooms...
            </div>
          ) : publicRooms.length > 0 ? (
            <div className="rf-flat-list">
              {publicRooms.map((room) => (
                <Link
                  key={room.room_id}
                  href={`/rooms/${room.room_id}`}
                  className="rf-flat-row flex items-center justify-between gap-4"
                >
                  <div className="min-w-0 space-y-1">
                    <p className="truncate font-semibold leading-snug">{room.title}</p>
                    <div className="flex items-center gap-3 text-xs muted">
                      <span>Hosted by {room.host_username}</span>
                      <span>{formatRoomMembersLabel(room.member_count)}</span>
                    </div>
                  </div>
                  <span className="rf-text-action shrink-0 text-sm">Join</span>
                </Link>
              ))}
            </div>
          ) : null}
          {!loadingData && publicRooms.length === 0 ? (
            <div className="space-y-3 text-sm text-white/88">
              <p>Jump into what&apos;s live right now, or open the full rooms workspace.</p>
              <Link href="/rooms" className="rf-text-action mt-3 text-sm">
                Join an active room
              </Link>
            </div>
          ) : null}
        </section>
      </div>

      <section className="rf-flat-section">
        <div className="flex items-end justify-between gap-3">
          <div>
            <h2 className="text-xl font-semibold sm:text-2xl">Calendar: Next 7 Days</h2>
          </div>
          <Link href="/calendar" className="rf-text-action text-sm">
            Full calendar
          </Link>
        </div>
        {loadingData ? (
          <div className="rf-flat-empty text-sm muted" aria-live="polite">
            Loading upcoming events...
          </div>
        ) : !hasUpcomingCalendarEvents ? (
          <div className="rf-flat-empty text-sm muted">
            Nothing scheduled for the next 7 days.
          </div>
        ) : (
          <ul className="rf-flat-list">
              {nextSevenDayCalendar.map((day) => (
                <li key={`home-calendar-${day.dateKey}`} className="rf-flat-row">
                  <div className="flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between">
                    <div>
                      <p className="text-sm font-semibold">{day.dateLabel}</p>
                      <p className="text-xs muted">
                        {day.events.length === 0
                          ? 'No events'
                          : `${day.events.length} event${day.events.length === 1 ? '' : 's'}`}
                      </p>
                    </div>
                    {day.events.length > 0 ? (
                      <div className="space-y-1 sm:max-w-[70%]">
                        {day.events.slice(0, 3).map((event) => (
                          <p
                            key={`home-calendar-event-${day.dateKey}-${event.occurrence_id}`}
                            className="text-sm sm:text-right"
                          >
                            {event.title}
                            {event.scope === 'global' ? ' · Global' : ''}
                          </p>
                        ))}
                        {day.events.length > 3 ? (
                          <p className="text-right text-xs muted">+{day.events.length - 3} more</p>
                        ) : null}
                      </div>
                    ) : null}
                  </div>
                </li>
              ))}
          </ul>
        )}
      </section>
    </div>
  );
}
