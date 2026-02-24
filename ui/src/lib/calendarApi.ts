import { apiJson } from './api';

export type CalendarEventScope = 'global' | 'personal';
export type CalendarEventType = 'event' | 'birthday';
export type CalendarRecurrence = 'none' | 'yearly';

export interface CalendarEvent {
  id: string;
  occurrence_id: string;
  title: string;
  description: string | null;
  display_description: string | null;
  event_date: string;
  source_event_date: string;
  scope: CalendarEventScope;
  owner_user_id: string | null;
  owner_username: string | null;
  event_type: CalendarEventType;
  recurrence: CalendarRecurrence;
  birthday_year: number | null;
  derived_age: number | null;
  created_by_user_id: string;
  created_by_username: string | null;
  can_edit: boolean;
  can_delete: boolean;
}

export interface CalendarUser {
  id: string;
  username: string;
  role: string;
}

interface CalendarEventsEnvelope {
  events: CalendarEvent[];
}

export interface ListCalendarEventsParams {
  from: string;
  to: string;
  scope?: 'all' | CalendarEventScope;
}

export async function listCalendarEvents(params: ListCalendarEventsParams): Promise<CalendarEvent[]> {
  const query = new URLSearchParams({
    from: params.from,
    to: params.to,
  });
  if (params.scope) {
    query.set('scope', params.scope);
  }
  const res = await apiJson<CalendarEventsEnvelope>(`/calendar/events?${query.toString()}`);
  return res.events;
}

export async function listPersonalCalendarEventsForAdmin(
  params: ListCalendarEventsParams,
): Promise<CalendarEvent[]> {
  const query = new URLSearchParams({
    from: params.from,
    to: params.to,
  });
  const res = await apiJson<CalendarEventsEnvelope>(`/calendar/events/personal?${query.toString()}`);
  return res.events;
}

export interface CreateCalendarEventRequest {
  title: string;
  description?: string;
  event_date: string;
  scope?: CalendarEventScope;
  owner_user_id?: string;
  event_type?: CalendarEventType;
  recurrence?: CalendarRecurrence;
  birthday_year?: number;
}

export async function createCalendarEvent(body: CreateCalendarEventRequest): Promise<CalendarEvent> {
  return apiJson<CalendarEvent>('/calendar/events', {
    method: 'POST',
    body: JSON.stringify(body),
  });
}

export interface UpdateCalendarEventRequest {
  title?: string;
  description?: string;
  event_date?: string;
  scope?: CalendarEventScope;
  owner_user_id?: string;
  event_type?: CalendarEventType;
  recurrence?: CalendarRecurrence;
  birthday_year?: number;
}

export async function updateCalendarEvent(
  eventId: string,
  body: UpdateCalendarEventRequest,
): Promise<CalendarEvent> {
  return apiJson<CalendarEvent>(`/calendar/events/${eventId}`, {
    method: 'PATCH',
    body: JSON.stringify(body),
  });
}

export async function deleteCalendarEvent(eventId: string): Promise<void> {
  await apiJson<void>(`/calendar/events/${eventId}`, {
    method: 'DELETE',
  });
}

export async function listCalendarUsers(): Promise<CalendarUser[]> {
  return apiJson<CalendarUser[]>('/calendar/users');
}
