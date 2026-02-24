'use client';

import { useCallback, useEffect, useMemo, useState } from 'react';
import { useAuth } from '@/lib/auth';
import {
  createCalendarEvent,
  deleteCalendarEvent,
  listCalendarEvents,
  listCalendarUsers,
  listPersonalCalendarEventsForAdmin,
  updateCalendarEvent,
  type CreateCalendarEventRequest,
  type CalendarEvent,
  type CalendarEventScope,
  type CalendarRecurrence,
  type CalendarUser,
  type UpdateCalendarEventRequest,
} from '@/lib/calendarApi';

type CalendarView = 'month' | 'week' | 'next_week' | 'next_7_days' | 'agenda_30';

const WEEKDAY_LABELS = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];

function withNoon(date: Date): Date {
  const next = new Date(date);
  next.setHours(12, 0, 0, 0);
  return next;
}

function startOfWeek(date: Date): Date {
  const d = withNoon(date);
  const dayIndex = (d.getDay() + 6) % 7;
  d.setDate(d.getDate() - dayIndex);
  return d;
}

function addDays(date: Date, days: number): Date {
  const next = withNoon(date);
  next.setDate(next.getDate() + days);
  return next;
}

function addMonths(date: Date, months: number): Date {
  const next = withNoon(date);
  next.setMonth(next.getMonth() + months);
  return next;
}

function formatYmd(date: Date): string {
  const y = date.getFullYear();
  const m = String(date.getMonth() + 1).padStart(2, '0');
  const d = String(date.getDate()).padStart(2, '0');
  return `${y}-${m}-${d}`;
}

function parseYmd(raw: string): Date {
  const [y, m, d] = raw.split('-').map((part) => Number(part));
  return withNoon(new Date(y, (m || 1) - 1, d || 1));
}

function enumerateDays(from: Date, to: Date): Date[] {
  const out: Date[] = [];
  let cursor = withNoon(from);
  while (cursor <= to) {
    out.push(cursor);
    cursor = addDays(cursor, 1);
  }
  return out;
}

function rangeForView(view: CalendarView, anchorDate: Date): { from: Date; to: Date; days: Date[] } {
  const anchor = withNoon(anchorDate);
  if (view === 'month') {
    const monthStart = withNoon(new Date(anchor.getFullYear(), anchor.getMonth(), 1));
    const monthEnd = withNoon(new Date(anchor.getFullYear(), anchor.getMonth() + 1, 0));
    const from = startOfWeek(monthStart);
    const to = addDays(startOfWeek(monthEnd), 6);
    return { from, to, days: enumerateDays(from, to) };
  }
  if (view === 'week') {
    const from = startOfWeek(anchor);
    const to = addDays(from, 6);
    return { from, to, days: enumerateDays(from, to) };
  }
  if (view === 'next_week') {
    const from = addDays(startOfWeek(anchor), 7);
    const to = addDays(from, 6);
    return { from, to, days: enumerateDays(from, to) };
  }
  if (view === 'next_7_days') {
    const from = withNoon(new Date());
    const to = addDays(from, 6);
    return { from, to, days: enumerateDays(from, to) };
  }
  const from = withNoon(anchor);
  const to = addDays(from, 29);
  return { from, to, days: enumerateDays(from, to) };
}

function formatHeader(view: CalendarView, from: Date, to: Date): string {
  if (view === 'month') {
    return from.toLocaleDateString(undefined, { month: 'long', year: 'numeric' });
  }
  const fromLabel = from.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
  const toLabel = to.toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' });
  return `${fromLabel} — ${toLabel}`;
}

function nextAnchor(view: CalendarView, anchor: Date, direction: -1 | 1): Date {
  if (view === 'month') {
    return addMonths(anchor, direction);
  }
  if (view === 'agenda_30') {
    return addDays(anchor, direction * 30);
  }
  return addDays(anchor, direction * 7);
}

function dayCellLabel(date: Date): string {
  return date.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
}

function eventBadgeClass(event: CalendarEvent): string {
  if (event.event_type === 'birthday') {
    return 'border-pink-400/45 bg-pink-500/10';
  }
  if (event.scope === 'global') {
    return 'border-amber-300/45 bg-amber-500/10';
  }
  return 'border-[var(--border)] bg-white/5';
}

export default function CalendarPage() {
  const { me } = useAuth();
  const [view, setView] = useState<CalendarView>('month');
  const [anchorDate, setAnchorDate] = useState<Date>(withNoon(new Date()));
  const [events, setEvents] = useState<CalendarEvent[]>([]);
  const [adminPersonalEvents, setAdminPersonalEvents] = useState<CalendarEvent[]>([]);
  const [users, setUsers] = useState<CalendarUser[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [editingEventId, setEditingEventId] = useState<string | null>(null);

  const [title, setTitle] = useState('');
  const [description, setDescription] = useState('');
  const [eventDate, setEventDate] = useState(formatYmd(withNoon(new Date())));
  const [scope, setScope] = useState<CalendarEventScope>('personal');
  const [ownerUserId, setOwnerUserId] = useState('');
  const [eventType, setEventType] = useState<'event' | 'birthday'>('event');
  const [recurrence, setRecurrence] = useState<CalendarRecurrence>('none');
  const [birthdayYear, setBirthdayYear] = useState('');

  const { from, to, days } = useMemo(() => rangeForView(view, anchorDate), [view, anchorDate]);
  const fromYmd = useMemo(() => formatYmd(from), [from]);
  const toYmd = useMemo(() => formatYmd(to), [to]);
  const isAdmin = me?.role === 'admin';

  const eventsByDate = useMemo(() => {
    const byDate = new Map<string, CalendarEvent[]>();
    for (const event of events) {
      const key = event.event_date;
      const current = byDate.get(key) ?? [];
      current.push(event);
      byDate.set(key, current);
    }
    for (const [key, value] of byDate) {
      value.sort((a, b) => a.title.localeCompare(b.title));
      byDate.set(key, value);
    }
    return byDate;
  }, [events]);

  const reload = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [eventRows, userRows, personalRows] = await Promise.all([
        listCalendarEvents({ from: fromYmd, to: toYmd, scope: 'all' }),
        listCalendarUsers(),
        isAdmin ? listPersonalCalendarEventsForAdmin({ from: fromYmd, to: toYmd }) : Promise.resolve([]),
      ]);
      setEvents(eventRows);
      setUsers(userRows);
      setAdminPersonalEvents(personalRows);
      if (!ownerUserId) {
        const fallbackOwner = userRows.find((user) => user.id === me?.id)?.id ?? me?.id ?? '';
        setOwnerUserId(fallbackOwner);
      }
    } catch (err: any) {
      setError(err?.message || 'Failed to load calendar data');
    } finally {
      setLoading(false);
    }
  }, [fromYmd, toYmd, isAdmin, ownerUserId, me?.id]);

  useEffect(() => {
    void reload();
  }, [reload]);

  const resetForm = useCallback(() => {
    setEditingEventId(null);
    setTitle('');
    setDescription('');
    setEventDate(formatYmd(withNoon(new Date())));
    setScope('personal');
    setEventType('event');
    setRecurrence('none');
    setBirthdayYear('');
    setOwnerUserId(me?.id ?? '');
  }, [me?.id]);

  useEffect(() => {
    if (!ownerUserId && me?.id) {
      setOwnerUserId(me.id);
    }
  }, [ownerUserId, me?.id]);

  const onSave = async () => {
    setSaving(true);
    setError(null);
    try {
      const baseBody: CreateCalendarEventRequest = {
        title,
        description: description.trim() || undefined,
        event_date: eventDate,
      };

      if (isAdmin) {
        baseBody.scope = scope;
        if (scope === 'personal' && ownerUserId) {
          baseBody.owner_user_id = ownerUserId;
        }
      }

      if (eventType === 'birthday') {
        baseBody.event_type = 'birthday';
        baseBody.recurrence = 'yearly';
        baseBody.birthday_year = Number(birthdayYear);
      } else {
        baseBody.event_type = 'event';
        baseBody.recurrence = recurrence;
      }

      if (editingEventId) {
        const patchBody: UpdateCalendarEventRequest = {
          ...baseBody,
        };
        await updateCalendarEvent(editingEventId, patchBody);
      } else {
        await createCalendarEvent(baseBody);
      }

      resetForm();
      await reload();
    } catch (err: any) {
      setError(err?.message || 'Failed to save event');
    } finally {
      setSaving(false);
    }
  };

  const onDelete = async (eventId: string) => {
    setError(null);
    try {
      await deleteCalendarEvent(eventId);
      if (editingEventId === eventId) {
        resetForm();
      }
      await reload();
    } catch (err: any) {
      setError(err?.message || 'Failed to delete event');
    }
  };

  const onEdit = (event: CalendarEvent) => {
    if (!event.can_edit) return;
    setEditingEventId(event.id);
    setTitle(event.title);
    setDescription(event.description ?? '');
    setEventDate(event.source_event_date ?? event.event_date);
    setScope(event.scope);
    setEventType(event.event_type);
    setRecurrence(event.recurrence);
    setBirthdayYear(event.birthday_year ? String(event.birthday_year) : '');
    setOwnerUserId(event.owner_user_id ?? me?.id ?? '');
    window.scrollTo({ top: 0, behavior: 'smooth' });
  };

  return (
    <div className="space-y-6 animate-rise">
      <header className="space-y-2">
        <h1 className="text-3xl font-semibold sm:text-4xl">Calendar</h1>
        <p className="text-sm muted sm:text-base">
          Shared scheduling for your server. Admins can publish global events, and each user can keep private personal events.
        </p>
      </header>

      <div className="grid grid-cols-1 gap-4 xl:grid-cols-[2fr_1fr]">
        <section className="panel rounded-2xl p-4 sm:p-5 space-y-4">
          <div className="flex flex-wrap items-center gap-2 justify-between">
            <div className="flex items-center gap-2">
              <button
                type="button"
                className="btn-secondary px-3 py-1.5 text-sm"
                onClick={() => setAnchorDate((prev) => nextAnchor(view, prev, -1))}
              >
                Previous
              </button>
              <button
                type="button"
                className="btn-secondary px-3 py-1.5 text-sm"
                onClick={() => setAnchorDate(withNoon(new Date()))}
              >
                Today
              </button>
              <button
                type="button"
                className="btn-secondary px-3 py-1.5 text-sm"
                onClick={() => setAnchorDate((prev) => nextAnchor(view, prev, 1))}
              >
                Next
              </button>
            </div>
            <select
              className="select w-full sm:w-auto px-3 py-2 text-sm"
              value={view}
              onChange={(e) => setView(e.target.value as CalendarView)}
            >
              <option value="month">Monthly</option>
              <option value="week">This Week</option>
              <option value="next_week">Upcoming Week</option>
              <option value="next_7_days">Next 7 Days</option>
              <option value="agenda_30">Agenda (30 Days)</option>
            </select>
          </div>

          <div className="flex items-center justify-between">
            <h2 className="text-lg font-semibold">{formatHeader(view, from, to)}</h2>
            <span className="chip">
              {events.length} events
            </span>
          </div>

          {loading ? (
            <div className="panel-soft rounded-xl px-4 py-10 text-sm muted text-center">
              Loading calendar…
            </div>
          ) : view === 'agenda_30' ? (
            <div className="space-y-2 max-h-[36rem] overflow-y-auto pr-1">
              {days.map((day) => {
                const key = formatYmd(day);
                const dayEvents = eventsByDate.get(key) ?? [];
                return (
                  <div key={key} className="panel-soft rounded-xl px-3 py-2 space-y-2">
                    <p className="text-sm font-semibold">{day.toLocaleDateString(undefined, { weekday: 'long', month: 'short', day: 'numeric' })}</p>
                    {dayEvents.length === 0 ? (
                      <p className="text-xs muted">No events</p>
                    ) : (
                      <div className="space-y-1">
                        {dayEvents.map((event) => (
                          <div key={event.occurrence_id} className={`rounded-lg border px-2 py-1.5 text-xs ${eventBadgeClass(event)}`}>
                            <p className="font-medium">{event.title}</p>
                            {event.display_description && <p className="muted">{event.display_description}</p>}
                            {event.owner_username && event.scope === 'personal' && (
                              <p className="muted">Owner: {event.owner_username}</p>
                            )}
                            {(event.can_edit || event.can_delete) && (
                              <div className="mt-1 flex gap-2">
                                {event.can_edit && (
                                  <button type="button" className="btn-ghost px-2 py-0.5 text-xs" onClick={() => onEdit(event)}>
                                    Edit
                                  </button>
                                )}
                                {event.can_delete && (
                                  <button type="button" className="btn-ghost px-2 py-0.5 text-xs text-red-300" onClick={() => void onDelete(event.id)}>
                                    Delete
                                  </button>
                                )}
                              </div>
                            )}
                          </div>
                        ))}
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          ) : (
            <div className="space-y-2">
              <div className="grid grid-cols-7 gap-2">
                {WEEKDAY_LABELS.map((label) => (
                  <div key={label} className="rounded-lg border border-[var(--border)] bg-white/5 px-2 py-1 text-center text-xs font-semibold muted">
                    {label}
                  </div>
                ))}
              </div>
              <div className={`grid grid-cols-7 gap-2 ${view === 'month' ? 'auto-rows-[minmax(9rem,1fr)]' : 'auto-rows-[minmax(14rem,1fr)]'}`}>
                {days.map((day) => {
                  const key = formatYmd(day);
                  const dayEvents = eventsByDate.get(key) ?? [];
                  const outsideMonth = view === 'month' && day.getMonth() !== anchorDate.getMonth();
                  return (
                    <div
                      key={key}
                      className={`rounded-xl border px-2 py-2 overflow-hidden flex flex-col gap-2 ${outsideMonth ? 'border-[var(--border)]/60 opacity-70' : 'border-[var(--border)] bg-white/5'}`}
                    >
                      <div className="flex items-center justify-between">
                        <p className="text-xs font-semibold">{dayCellLabel(day)}</p>
                        <span className="text-[11px] muted">{dayEvents.length}</span>
                      </div>
                      <div className="space-y-1 overflow-y-auto pr-1">
                        {dayEvents.map((event) => (
                          <div
                            key={event.occurrence_id}
                            className={`rounded-lg border px-2 py-1.5 text-[11px] ${eventBadgeClass(event)}`}
                          >
                            <p className="font-semibold leading-tight">{event.title}</p>
                            {event.display_description && (
                              <p className="mt-0.5 muted leading-tight">{event.display_description}</p>
                            )}
                            {event.owner_username && event.scope === 'personal' && (
                              <p className="mt-0.5 muted leading-tight">Owner: {event.owner_username}</p>
                            )}
                            {(event.can_edit || event.can_delete) && (
                              <div className="mt-1 flex gap-1">
                                {event.can_edit && (
                                  <button
                                    type="button"
                                    className="btn-ghost px-1.5 py-0.5 text-[10px]"
                                    onClick={() => onEdit(event)}
                                  >
                                    Edit
                                  </button>
                                )}
                                {event.can_delete && (
                                  <button
                                    type="button"
                                    className="btn-ghost px-1.5 py-0.5 text-[10px] text-red-300"
                                    onClick={() => void onDelete(event.id)}
                                  >
                                    Delete
                                  </button>
                                )}
                              </div>
                            )}
                          </div>
                        ))}
                      </div>
                    </div>
                  );
                })}
              </div>
            </div>
          )}
        </section>

        <aside className="panel rounded-2xl p-4 sm:p-5 space-y-4">
          <div className="flex items-center justify-between gap-2">
            <h2 className="text-lg font-semibold">{editingEventId ? 'Edit Event' : 'Create Event'}</h2>
            {editingEventId && (
              <button type="button" className="btn-ghost px-2 py-1 text-xs" onClick={resetForm}>
                Cancel edit
              </button>
            )}
          </div>

          <div className="space-y-3">
            <input
              className="input px-3 py-2 text-sm"
              placeholder="Event title"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              maxLength={140}
            />

            <input
              type="date"
              className="input px-3 py-2 text-sm"
              value={eventDate}
              onChange={(e) => setEventDate(e.target.value)}
            />

            <textarea
              className="input min-h-24 resize-y px-3 py-2 text-sm"
              placeholder="Description (optional)"
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              maxLength={500}
            />

            <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
              <label className="space-y-1">
                <p className="text-xs uppercase tracking-[0.14em] muted">Type</p>
                <select
                  className="select px-3 py-2 text-sm"
                  value={eventType}
                  onChange={(e) => setEventType(e.target.value as 'event' | 'birthday')}
                >
                  <option value="event">Event</option>
                  <option value="birthday">Birthday</option>
                </select>
              </label>

              {eventType === 'event' ? (
                <label className="space-y-1">
                  <p className="text-xs uppercase tracking-[0.14em] muted">Repeat</p>
                  <select
                    className="select px-3 py-2 text-sm"
                    value={recurrence}
                    onChange={(e) => setRecurrence(e.target.value as CalendarRecurrence)}
                  >
                    <option value="none">One-time</option>
                    <option value="yearly">Yearly</option>
                  </select>
                </label>
              ) : (
                <label className="space-y-1">
                  <p className="text-xs uppercase tracking-[0.14em] muted">Birth Year</p>
                  <input
                    type="number"
                    className="input px-3 py-2 text-sm"
                    placeholder="1994"
                    value={birthdayYear}
                    onChange={(e) => setBirthdayYear(e.target.value)}
                    min={1900}
                    max={new Date().getFullYear()}
                  />
                </label>
              )}
            </div>

            {isAdmin && (
              <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
                <label className="space-y-1">
                  <p className="text-xs uppercase tracking-[0.14em] muted">Scope</p>
                  <select
                    className="select px-3 py-2 text-sm"
                    value={scope}
                    onChange={(e) => setScope(e.target.value as CalendarEventScope)}
                  >
                    <option value="personal">Personal</option>
                    <option value="global">Global (everyone)</option>
                  </select>
                </label>
                <label className="space-y-1">
                  <p className="text-xs uppercase tracking-[0.14em] muted">Owner</p>
                  <select
                    className="select px-3 py-2 text-sm"
                    value={ownerUserId}
                    disabled={scope === 'global'}
                    onChange={(e) => setOwnerUserId(e.target.value)}
                  >
                    {users.map((user) => (
                      <option key={user.id} value={user.id}>
                        {user.username}
                      </option>
                    ))}
                  </select>
                </label>
              </div>
            )}

            <button
              type="button"
              className="btn-primary w-full px-4 py-2.5 text-sm disabled:opacity-60"
              disabled={
                saving ||
                !title.trim() ||
                !eventDate ||
                (eventType === 'birthday' && !birthdayYear.trim())
              }
              onClick={() => void onSave()}
            >
              {saving ? 'Saving…' : editingEventId ? 'Save Changes' : 'Create Event'}
            </button>
          </div>

          {isAdmin && (
            <div className="space-y-2">
              <h3 className="text-sm font-semibold">Personal Events (Admin View)</h3>
              <div className="max-h-72 overflow-y-auto space-y-2 pr-1">
                {adminPersonalEvents.length === 0 ? (
                  <div className="panel-soft rounded-xl px-3 py-2 text-xs muted">No personal events in this range.</div>
                ) : (
                  adminPersonalEvents.map((event) => (
                    <div key={event.occurrence_id} className="panel-soft rounded-xl px-3 py-2 text-xs">
                      <p className="font-semibold">{event.title}</p>
                      <p className="muted">{event.event_date} · {event.owner_username ?? 'Unknown owner'}</p>
                    </div>
                  ))
                )}
              </div>
            </div>
          )}

          {error && (
            <div className="rounded-xl border border-red-400/40 bg-red-500/10 px-3 py-2 text-xs text-red-200">
              {error}
            </div>
          )}
        </aside>
      </div>
    </div>
  );
}
