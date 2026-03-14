'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
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
import { playTelegramDeleteAnimation } from '@/lib/deleteAnimation';
import { clientErrorMessage } from '@/lib/errors';

type CalendarView = 'month' | 'week' | 'next_week' | 'next_7_days' | 'agenda_30' | 'events_30';
type CalendarSidePanelMode = 'closed' | 'editor' | 'day';

const WEEKDAY_LABELS = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];
const YMD_PREFIX_RE = /^(\d{4}-\d{2}-\d{2})/;

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

function coerceYmd(raw: string): string | null {
  const trimmed = raw.trim();
  const direct = YMD_PREFIX_RE.exec(trimmed);
  if (direct) {
    return direct[1];
  }
  const parsed = new Date(trimmed);
  if (Number.isNaN(parsed.getTime())) {
    return null;
  }
  return formatYmd(withNoon(parsed));
}

function parseYmd(value: string): Date {
  const [year, month, day] = value.split('-').map(Number);
  return withNoon(new Date(year, (month || 1) - 1, day || 1));
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

function formatHeader(view: CalendarView, from: Date, to: Date, anchor: Date): string {
  if (view === 'month') {
    return anchor.toLocaleDateString(undefined, { month: 'long', year: 'numeric' });
  }
  if (view === 'events_30') {
    return 'Events in the Next 30 Days';
  }
  const fromLabel = from.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
  const toLabel = to.toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' });
  return `${fromLabel} — ${toLabel}`;
}

function nextAnchor(view: CalendarView, anchor: Date, direction: -1 | 1): Date {
  if (view === 'month') {
    return addMonths(anchor, direction);
  }
  if (view === 'agenda_30' || view === 'events_30') {
    return addDays(anchor, direction * 30);
  }
  return addDays(anchor, direction * 7);
}

function dayCellLabel(date: Date): string {
  return date.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
}

function dayCellDay(date: Date): string {
  return date.toLocaleDateString(undefined, { day: 'numeric' });
}

function dayCellMonth(date: Date): string {
  return date.toLocaleDateString(undefined, { month: 'short' });
}

function sameCalendarDay(a: Date, b: Date): boolean {
  return (
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate()
  );
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
  const [sidePanelMode, setSidePanelMode] = useState<CalendarSidePanelMode>('closed');
  const [selectedDayYmd, setSelectedDayYmd] = useState<string | null>(null);
  const [monthViewCondensed, setMonthViewCondensed] = useState(false);
  const monthGridRef = useRef<HTMLDivElement | null>(null);

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
  const today = useMemo(() => withNoon(new Date()), []);
  const todayWeekdayIndex = useMemo(() => (today.getDay() + 6) % 7, [today]);
  const weekdayHeaders = useMemo(() => {
    if (view === 'next_7_days') {
      return days.slice(0, 7).map((day) => ({
        label: day.toLocaleDateString(undefined, { weekday: 'short' }),
        isToday: sameCalendarDay(day, today),
      }));
    }
    return WEEKDAY_LABELS.map((label, index) => ({
      label,
      isToday: index === todayWeekdayIndex,
    }));
  }, [view, days, today, todayWeekdayIndex]);

  const eventsByDate = useMemo(() => {
    const byDate = new Map<string, CalendarEvent[]>();
    for (const event of events) {
      const key = coerceYmd(event.event_date) ?? coerceYmd(event.source_event_date) ?? event.event_date;
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
  const selectedDayDate = useMemo(
    () => (selectedDayYmd ? parseYmd(selectedDayYmd) : null),
    [selectedDayYmd],
  );
  const selectedDayEvents = useMemo(
    () => (selectedDayYmd ? eventsByDate.get(selectedDayYmd) ?? [] : []),
    [eventsByDate, selectedDayYmd],
  );
  const panelOpen = sidePanelMode !== 'closed';
  const eventPanelButtonLabel =
    sidePanelMode === 'editor'
      ? 'Hide Event Panel ▴'
      : sidePanelMode === 'day'
        ? 'Open Event Panel ▸'
        : 'Show Event Panel ▾';

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
    } catch (err: unknown) {
      setError(clientErrorMessage(err, 'Failed to load calendar data'));
    } finally {
      setLoading(false);
    }
  }, [fromYmd, toYmd, isAdmin, ownerUserId, me?.id]);

  useEffect(() => {
    void reload();
  }, [reload]);

  useEffect(() => {
    if (view === 'week' || view === 'next_week' || view === 'next_7_days') {
      setAnchorDate(withNoon(new Date()));
    }
  }, [view]);

  useEffect(() => {
    if (view !== 'month') {
      setMonthViewCondensed(false);
      if (sidePanelMode === 'day') {
        setSidePanelMode('closed');
        setSelectedDayYmd(null);
      }
      return;
    }

    const node = monthGridRef.current;
    if (!node || typeof ResizeObserver === 'undefined') {
      return;
    }

    const updateDensity = () => {
      const rowHeight = node.getBoundingClientRect().height / 6;
      setMonthViewCondensed(rowHeight < 118);
    };

    updateDensity();
    const observer = new ResizeObserver(updateDensity);
    observer.observe(node);
    return () => observer.disconnect();
  }, [loading, view, sidePanelMode]);

  useEffect(() => {
    if (view !== 'month' || sidePanelMode !== 'day' || !selectedDayYmd) {
      return;
    }
    const selectedDayStillVisible = days.some((day) => formatYmd(day) === selectedDayYmd);
    if (!selectedDayStillVisible) {
      setSidePanelMode('closed');
      setSelectedDayYmd(null);
    }
  }, [days, selectedDayYmd, sidePanelMode, view]);

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
    } catch (err: unknown) {
      setError(clientErrorMessage(err, 'Failed to save event'));
    } finally {
      setSaving(false);
    }
  };

  const onDelete = async (eventId: string, target?: HTMLElement | null) => {
    setError(null);
    try {
      await playTelegramDeleteAnimation(target);
      await deleteCalendarEvent(eventId);
      if (editingEventId === eventId) {
        resetForm();
      }
      await reload();
    } catch (err: unknown) {
      setError(clientErrorMessage(err, 'Failed to delete event'));
    }
  };

  const onEdit = (event: CalendarEvent) => {
    if (!event.can_edit) return;
    setSidePanelMode('editor');
    setEditingEventId(event.id);
    setTitle(event.title);
    setDescription(event.description ?? '');
    setEventDate(
      coerceYmd(event.source_event_date) ??
        coerceYmd(event.event_date) ??
        formatYmd(withNoon(new Date())),
    );
    setScope(event.scope);
    setEventType(event.event_type);
    setRecurrence(event.recurrence);
    setBirthdayYear(event.birthday_year ? String(event.birthday_year) : '');
    setOwnerUserId(event.owner_user_id ?? me?.id ?? '');
  };

  const openCreatePanelForDate = useCallback(
    (dateYmd?: string) => {
      resetForm();
      if (dateYmd) {
        setEventDate(dateYmd);
      }
      setSidePanelMode('editor');
    },
    [resetForm],
  );

  const openDayPanel = useCallback((dateYmd: string) => {
    setSelectedDayYmd(dateYmd);
    setSidePanelMode('day');
  }, []);

  return (
    <div className="space-y-6 animate-rise">
      <header className="space-y-2">
        <h1 className="text-3xl font-semibold sm:text-4xl">Calendar</h1>
        <p className="text-sm muted sm:text-base">
          Shared scheduling for your server. Admins can publish global events, and each user can keep private personal events.
        </p>
        <div>
          <button
            type="button"
            className="btn-secondary px-3 py-1.5 text-sm"
            onClick={() =>
              setSidePanelMode((prev) => (prev === 'editor' ? 'closed' : 'editor'))
            }
          >
            {eventPanelButtonLabel}
          </button>
        </div>
      </header>

      <div
        className={`grid grid-cols-1 gap-4 min-h-[40rem] lg:h-[calc(100dvh-11.5rem)] ${
          panelOpen ? 'lg:grid-cols-[minmax(0,1.6fr)_minmax(20rem,1fr)]' : ''
        }`}
      >
        <section className="panel rounded-2xl p-4 sm:p-5 space-y-4 flex flex-col lg:h-full lg:min-h-0">
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
              <option value="events_30">Events Only (30 Days)</option>
            </select>
          </div>

          <div className="flex items-center justify-between">
            <h2 className="text-lg font-semibold">{formatHeader(view, from, to, anchorDate)}</h2>
            <span className="chip">
              {events.length} events
            </span>
          </div>

          <div className="flex-1 min-h-0">
            {loading ? (
              <div
                className="panel-soft rounded-xl h-full px-4 py-10 text-sm muted text-center flex items-center justify-center"
                role="status"
                aria-live="polite"
              >
                Loading calendar…
              </div>
            ) : view === 'agenda_30' || view === 'events_30' ? (
              <div className="h-full space-y-2 overflow-y-auto pr-1">
                {(() => {
                  const dayRows = days
                    .map((day) => {
                      const key = formatYmd(day);
                      const dayEvents = eventsByDate.get(key) ?? [];
                      return { day, key, dayEvents };
                    })
                    .filter((row) => view !== 'events_30' || row.dayEvents.length > 0);

                  if (dayRows.length === 0) {
                    return (
                      <div className="panel-soft rounded-xl px-4 py-4 text-sm muted">
                        No events in this 30-day window.
                      </div>
                    );
                  }

                  return dayRows.map(({ day, key, dayEvents }) => {
                    const isToday = sameCalendarDay(day, today);
                    return (
                    <div
                      key={key}
                      className={`panel-soft rounded-xl px-3 py-2 space-y-2 border ${
                        isToday
                          ? 'calendar-today-outline border-transparent'
                          : 'border-[var(--border)]'
                      }`}
                    >
                      <p className="text-sm font-semibold">{day.toLocaleDateString(undefined, { weekday: 'long', month: 'short', day: 'numeric' })}</p>
                      {dayEvents.length === 0 ? (
                        <p className="text-xs muted">No events</p>
                      ) : (
                        <div className="space-y-1">
                          {dayEvents.map((event) => (
                            <div
                              key={event.occurrence_id}
                              data-calendar-event-id={event.id}
                              className={`rounded-lg border px-2 py-1.5 text-xs ${eventBadgeClass(event)}`}
                            >
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
                                    <button
                                      type="button"
                                      className="btn-ghost px-2 py-0.5 text-xs text-red-300"
                                      onClick={(e) =>
                                        void onDelete(
                                          event.id,
                                          (e.currentTarget as HTMLElement).closest('[data-calendar-event-id]') as HTMLElement | null,
                                        )
                                      }
                                    >
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
                  )});
                })()}
              </div>
            ) : view !== 'month' ? (
              <>
                <div className="space-y-2 h-full flex flex-col min-h-0 sm:hidden">
                  <div className="space-y-2 overflow-y-auto pr-1">
                    {days.map((day) => {
                      const key = formatYmd(day);
                      const dayEvents = eventsByDate.get(key) ?? [];
                      const isToday = sameCalendarDay(day, today);
                      return (
                        <div
                          key={key}
                          className={`panel-soft rounded-xl px-3 py-2 space-y-2 border ${
                            isToday
                              ? 'calendar-today-outline border-transparent'
                              : 'border-[var(--border)]'
                          }`}
                        >
                          <div className="flex items-start justify-between gap-3">
                            <div>
                              <p className="text-sm font-semibold">
                                {day.toLocaleDateString(undefined, { weekday: 'long' })}
                              </p>
                              <p className="text-xs muted">
                                {day.toLocaleDateString(undefined, { month: 'short', day: 'numeric' })}
                              </p>
                            </div>
                            <span className="text-xs muted">{dayEvents.length} events</span>
                          </div>
                          {dayEvents.length === 0 ? (
                            <p className="text-xs muted">No events</p>
                          ) : (
                            <div className="space-y-1">
                              {dayEvents.map((event) => (
                                <div
                                  key={event.occurrence_id}
                                  data-calendar-event-id={event.id}
                                  className={`rounded-lg border px-2 py-1.5 text-xs ${eventBadgeClass(event)}`}
                                >
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
                                        <button
                                          type="button"
                                          className="btn-ghost px-2 py-0.5 text-xs text-red-300"
                                          onClick={(e) =>
                                            void onDelete(
                                              event.id,
                                              (e.currentTarget as HTMLElement).closest('[data-calendar-event-id]') as HTMLElement | null,
                                            )
                                          }
                                        >
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
                </div>
                <div className="hidden space-y-2 h-full flex-col min-h-0 sm:flex">
                  <div className="grid grid-cols-7 gap-2">
                    {weekdayHeaders.map(({ label, isToday }) => (
                      <div
                        key={label}
                        className={`rounded-lg border px-2 py-1 text-center text-xs font-semibold ${
                          isToday
                            ? 'border-white/30 bg-gradient-to-r from-[var(--orange)] via-[var(--danger)] to-[var(--purple-strong)] text-white'
                            : 'border-[var(--border)] bg-white/5 muted'
                        }`}
                      >
                        {label}
                      </div>
                    ))}
                  </div>
                  <div className="grid grid-cols-7 gap-2 flex-1 min-h-0 grid-rows-1">
                    {days.map((day) => {
                      const key = formatYmd(day);
                      const dayEvents = eventsByDate.get(key) ?? [];
                      const isToday = sameCalendarDay(day, today);
                      return (
                        <div
                          key={key}
                          className={`rounded-xl border px-2 py-2 overflow-hidden flex flex-col h-full min-h-0 gap-2 ${
                            isToday
                              ? 'calendar-today-outline border-transparent bg-white/[0.08]'
                              : 'border-[var(--border)] bg-white/5'
                          }`}
                        >
                          <div className="flex items-center justify-between">
                            <p className="text-xs font-semibold">{dayCellLabel(day)}</p>
                            <span className="text-[11px] muted">{dayEvents.length}</span>
                          </div>
                          <div className="space-y-1 overflow-y-auto pr-1 min-h-0">
                            {dayEvents.map((event) => (
                              <div
                                key={event.occurrence_id}
                                data-calendar-event-id={event.id}
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
                                        onClick={(e) =>
                                          void onDelete(
                                            event.id,
                                            (e.currentTarget as HTMLElement).closest('[data-calendar-event-id]') as HTMLElement | null,
                                          )
                                        }
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
              </>
            ) : (
              <div className="space-y-2 h-full flex flex-col min-h-0">
                <div className="grid grid-cols-7 gap-2">
                  {weekdayHeaders.map(({ label, isToday }) => (
                    <div
                      key={label}
                      className={`rounded-lg border px-2 py-1 text-center text-xs font-semibold ${
                        isToday
                          ? 'border-white/30 bg-gradient-to-r from-[var(--orange)] via-[var(--danger)] to-[var(--purple-strong)] text-white'
                          : 'border-[var(--border)] bg-white/5 muted'
                      }`}
                    >
                      {label}
                    </div>
                  ))}
                </div>
                <div
                  ref={monthGridRef}
                  className={`grid grid-cols-7 gap-2 flex-1 min-h-0 ${view === 'month' ? 'grid-rows-6' : 'grid-rows-1'}`}
                >
                  {days.map((day) => {
                    const key = formatYmd(day);
                    const dayEvents = eventsByDate.get(key) ?? [];
                    const outsideMonth = view === 'month' && day.getMonth() !== anchorDate.getMonth();
                    const isToday = sameCalendarDay(day, today);
                    const condensedCountLabel =
                      dayEvents.length === 0
                        ? 'No events'
                        : `${dayEvents.length} event${dayEvents.length === 1 ? '' : 's'}`;
                    return (
                      <div
                        key={key}
                        className={`rounded-xl border px-2 ${view === 'month' ? 'py-3' : 'py-2'} overflow-hidden flex flex-col h-full min-h-0 gap-2 ${
                          isToday
                            ? 'calendar-today-outline border-transparent bg-white/[0.08]'
                            : outsideMonth
                              ? 'border-[var(--border)]/60 opacity-70'
                              : 'border-[var(--border)] bg-white/5'
                        } cursor-pointer transition hover:border-white/20 hover:bg-white/[0.08]`}
                        onClick={(event) => {
                          const target = event.target as HTMLElement | null;
                          if (target?.closest('button, a, input, select, textarea, [role="button"]')) {
                            return;
                          }
                          openDayPanel(key);
                        }}
                        role={monthViewCondensed ? 'button' : undefined}
                        tabIndex={monthViewCondensed ? 0 : undefined}
                        onKeyDown={
                          monthViewCondensed
                            ? (event) => {
                                if (event.key === 'Enter' || event.key === ' ') {
                                  event.preventDefault();
                                  openDayPanel(key);
                                }
                              }
                            : undefined
                        }
                      >
                        <div className="sm:hidden">
                          <p className="text-xs font-semibold leading-tight">{dayCellDay(day)}</p>
                          <p className="text-[11px] muted leading-tight">{dayCellMonth(day)}</p>
                          <span className="block text-[11px] muted leading-tight">{dayEvents.length}</span>
                        </div>
                        <div className="hidden items-center justify-between sm:flex">
                          <p className="text-xs font-semibold">{dayCellLabel(day)}</p>
                          <span className="text-[11px] muted">{dayEvents.length}</span>
                        </div>
                        {monthViewCondensed ? (
                          <div className="mt-auto rounded-lg border border-[var(--border)] bg-black/10 px-2 py-1.5 text-[11px]">
                            <p className="font-medium leading-tight">{condensedCountLabel}</p>
                          </div>
                        ) : (
                          <div className="space-y-1 overflow-y-auto pr-1 min-h-0">
                            {dayEvents.map((event) => (
                              <div
                                key={event.occurrence_id}
                                data-calendar-event-id={event.id}
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
                                        onClick={(e) =>
                                          void onDelete(
                                            event.id,
                                            (e.currentTarget as HTMLElement).closest('[data-calendar-event-id]') as HTMLElement | null,
                                          )
                                        }
                                      >
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
              </div>
            )}
          </div>
        </section>

        {panelOpen && (
          <aside className="panel rounded-2xl p-4 sm:p-5 space-y-4 lg:h-full lg:min-h-0 lg:overflow-y-auto">
            {sidePanelMode === 'day' ? (
              <>
                <div className="flex items-center justify-between gap-2">
                  <div>
                    <h2 className="text-lg font-semibold">
                      {selectedDayDate
                        ? selectedDayDate.toLocaleDateString(undefined, {
                            weekday: 'long',
                            month: 'long',
                            day: 'numeric',
                            year: 'numeric',
                          })
                        : 'Day Overview'}
                    </h2>
                    <p className="text-sm muted">
                      {selectedDayEvents.length === 0
                        ? 'No events planned for this day.'
                        : `${selectedDayEvents.length} planned event${selectedDayEvents.length === 1 ? '' : 's'}.`}
                    </p>
                  </div>
                  <button
                    type="button"
                    className="btn-ghost px-2 py-1 text-xs"
                    onClick={() => setSidePanelMode('closed')}
                  >
                    Close
                  </button>
                </div>

                <button
                  type="button"
                  className="btn-primary w-full px-4 py-2.5 text-sm"
                  onClick={() => openCreatePanelForDate(selectedDayYmd ?? undefined)}
                >
                  Create Event For This Day
                </button>

                <div className="space-y-2">
                  {selectedDayEvents.length === 0 ? (
                    <div className="panel-soft rounded-xl px-4 py-4 text-sm muted">
                      This day is clear.
                    </div>
                  ) : (
                    selectedDayEvents.map((event) => (
                      <div
                        key={event.occurrence_id}
                        data-calendar-event-id={event.id}
                        className={`rounded-xl border px-3 py-3 text-sm ${eventBadgeClass(event)}`}
                      >
                        <div className="flex items-start justify-between gap-3">
                          <div className="space-y-1">
                            <p className="font-semibold">{event.title}</p>
                            {event.display_description && (
                              <p className="muted">{event.display_description}</p>
                            )}
                            {event.owner_username && event.scope === 'personal' && (
                              <p className="muted">Owner: {event.owner_username}</p>
                            )}
                            <p className="text-xs muted">
                              {event.event_type === 'birthday'
                                ? 'Birthday'
                                : event.scope === 'global'
                                  ? 'Global event'
                                  : 'Personal event'}
                            </p>
                          </div>
                        </div>
                        {(event.can_edit || event.can_delete) && (
                          <div className="mt-3 flex gap-2">
                            {event.can_edit && (
                              <button
                                type="button"
                                className="btn-ghost px-3 py-1 text-xs"
                                onClick={() => onEdit(event)}
                              >
                                Edit
                              </button>
                            )}
                            {event.can_delete && (
                              <button
                                type="button"
                                className="btn-ghost px-3 py-1 text-xs text-red-300"
                                onClick={(e) =>
                                  void onDelete(
                                    event.id,
                                    (e.currentTarget as HTMLElement).closest('[data-calendar-event-id]') as HTMLElement | null,
                                  )
                                }
                              >
                                Delete
                              </button>
                            )}
                          </div>
                        )}
                      </div>
                    ))
                  )}
                </div>
              </>
            ) : (
              <>
                <div className="flex items-center justify-between gap-2">
                  <h2 className="text-lg font-semibold">
                    {editingEventId ? 'Edit Event' : 'Create Event'}
                  </h2>
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
                            <p className="muted">
                              {coerceYmd(event.event_date) ?? event.event_date} ·{' '}
                              {event.owner_username ?? 'Unknown owner'}
                            </p>
                          </div>
                        ))
                      )}
                    </div>
                  </div>
                )}
              </>
            )}

            {error && (
              <div className="rounded-xl border border-red-400/40 bg-red-500/10 px-3 py-2 text-xs text-red-200">
                {error}
              </div>
            )}
          </aside>
        )}
      </div>
    </div>
  );
}
