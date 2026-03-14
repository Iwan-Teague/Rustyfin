'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import Link from 'next/link';
import SurfaceTabsBar from '@/app/components/SurfaceTabsBar';
import { clientErrorMessage } from '@/lib/errors';
import {
  listAdminDiagnosticNotifications,
  type AdminDiagnosticNotification,
} from '@/lib/notificationsApi';
import {
  declineWatchPartyInvite,
  listWatchPartyInvites,
  type WatchPartyInvite,
} from '@/lib/watchPartyApi';
import { roleLabel } from '@/lib/watchPartyRoles';

type NotificationInbox = 'user' | 'admin';

type Props = {
  isAdmin: boolean;
  className?: string;
};

function formatRelativeTime(tsSeconds: number): string {
  const deltaSeconds = Math.max(0, Math.floor(Date.now() / 1000) - Math.floor(tsSeconds));
  if (deltaSeconds < 60) return 'Just now';
  if (deltaSeconds < 3600) {
    const minutes = Math.floor(deltaSeconds / 60);
    return minutes === 1 ? '1 minute ago' : `${minutes} minutes ago`;
  }
  if (deltaSeconds < 86400) {
    const hours = Math.floor(deltaSeconds / 3600);
    return hours === 1 ? '1 hour ago' : `${hours} hours ago`;
  }
  const days = Math.floor(deltaSeconds / 86400);
  return days === 1 ? '1 day ago' : `${days} days ago`;
}

function formatAbsoluteTime(tsSeconds: number): string {
  if (!Number.isFinite(tsSeconds) || tsSeconds <= 0) return 'Unknown time';
  return new Date(tsSeconds * 1000).toLocaleString();
}

function kindLabel(kind: AdminDiagnosticNotification['kind']): string {
  switch (kind) {
    case 'scan_complete':
      return 'Scan';
    case 'library_created':
      return 'Library';
    case 'user_created':
      return 'User';
    default:
      return 'Admin';
  }
}

export default function NotificationsPopover({ isAdmin, className = '' }: Props) {
  const rootRef = useRef<HTMLDivElement | null>(null);
  const [open, setOpen] = useState(false);
  const [activeInbox, setActiveInbox] = useState<NotificationInbox>('user');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [invites, setInvites] = useState<WatchPartyInvite[]>([]);
  const [adminNotifications, setAdminNotifications] = useState<AdminDiagnosticNotification[]>([]);
  const [decliningRoomId, setDecliningRoomId] = useState<string | null>(null);

  const totalCount = invites.length + (isAdmin ? adminNotifications.length : 0);
  const panelLabel = isAdmin
    ? activeInbox === 'admin'
      ? 'Admin notifications'
      : 'User notifications'
    : 'Notifications';

  const refreshNotifications = useCallback(async () => {
    setLoading(true);
    setError(null);

    const userResult = await listWatchPartyInvites()
      .then((data) => ({ ok: true as const, data }))
      .catch((err: unknown) => ({ ok: false as const, err }));

    const adminResult = isAdmin
      ? await listAdminDiagnosticNotifications(24)
          .then((data) => ({ ok: true as const, data }))
          .catch((err: unknown) => ({ ok: false as const, err }))
      : ({ ok: true as const, data: [] as AdminDiagnosticNotification[] });

    if (userResult.ok) {
      setInvites(userResult.data);
    }
    if (adminResult.ok) {
      setAdminNotifications(adminResult.data);
    }

    if (!userResult.ok && !adminResult.ok) {
      setError(clientErrorMessage(userResult.err, 'Failed to load notifications'));
    } else if (!userResult.ok) {
      setError(clientErrorMessage(userResult.err, 'Failed to load user notifications'));
    } else if (!adminResult.ok) {
      setError(clientErrorMessage(adminResult.err, 'Failed to load admin notifications'));
    }

    setLoading(false);
  }, [isAdmin]);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: MouseEvent) => {
      const target = event.target as Node | null;
      if (target && rootRef.current && !rootRef.current.contains(target)) {
        setOpen(false);
      }
    };
    const onEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setOpen(false);
    };
    document.addEventListener('mousedown', onPointerDown);
    document.addEventListener('keydown', onEscape);
    return () => {
      document.removeEventListener('mousedown', onPointerDown);
      document.removeEventListener('keydown', onEscape);
    };
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const timer = window.setInterval(() => {
      void refreshNotifications();
    }, 45000);
    return () => window.clearInterval(timer);
  }, [open, refreshNotifications]);

  const handleToggleOpen = useCallback(() => {
    if (open) {
      setOpen(false);
      return;
    }
    setActiveInbox('user');
    setOpen(true);
    void refreshNotifications();
  }, [open, refreshNotifications]);

  const handleDeclineInvite = useCallback(async (roomId: string) => {
    setDecliningRoomId(roomId);
    try {
      await declineWatchPartyInvite(roomId);
      setInvites((prev) => prev.filter((invite) => invite.room_id !== roomId));
    } catch (err: unknown) {
      setError(clientErrorMessage(err, 'Failed to decline invite'));
    } finally {
      setDecliningRoomId(null);
    }
  }, []);

  const inboxBadges = useMemo(
    () => [`User ${invites.length}`, `Admin ${adminNotifications.length}`],
    [invites.length, adminNotifications.length],
  );

  return (
    <div ref={rootRef} className={`relative shrink-0 ${className}`.trim()}>
      <button
        type="button"
        className="btn-ghost relative h-11 w-11 p-0"
        aria-label="Open notifications"
        aria-haspopup="dialog"
        aria-expanded={open}
        onClick={handleToggleOpen}
      >
        <svg viewBox="0 0 24 24" className="h-5 w-5" fill="none" aria-hidden="true">
          <path
            d="M6 9a6 6 0 1 1 12 0v4l1.5 2.5H4.5L6 13V9Z"
            stroke="currentColor"
            strokeWidth="1.8"
            strokeLinejoin="round"
          />
          <path d="M10 18a2 2 0 0 0 4 0" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
        </svg>
        {totalCount > 0 ? (
          <span className="absolute -right-0.5 -top-0.5 inline-flex h-5 min-w-5 items-center justify-center rounded-full bg-[var(--orange-soft)] px-1 text-[10px] font-semibold text-black">
            {Math.min(totalCount, 99)}
          </span>
        ) : null}
      </button>

      {open ? (
        <div
          role="dialog"
          aria-label={panelLabel}
          className="rf-overlay-enter absolute right-0 top-[calc(100%+0.6rem)] z-[85] w-[min(30rem,calc(100vw-1.25rem))] panel rounded-2xl border border-[var(--border)] p-4 shadow-[0_16px_48px_rgba(0,0,0,0.48)]"
        >
          <div className="mb-3 flex items-center justify-between gap-3">
            <div>
              <h2 className="text-base font-semibold">Notifications</h2>
              <p className="text-xs muted">
                {isAdmin
                  ? 'User and admin inboxes. User opens by default.'
                  : 'Invites and account updates.'}
              </p>
            </div>
            <button
              type="button"
              onClick={() => void refreshNotifications()}
              className="btn-ghost px-3 py-1.5 text-xs"
              disabled={loading}
            >
              {loading ? 'Refreshing…' : 'Refresh'}
            </button>
          </div>

          {isAdmin ? (
            <SurfaceTabsBar<NotificationInbox>
              activeKey={activeInbox}
              onSelect={setActiveInbox}
              options={[
                { key: 'user', label: 'User' },
                { key: 'admin', label: 'Admin' },
              ]}
              badges={inboxBadges}
              className="mb-3"
            />
          ) : null}

          {error ? (
            <div className="notice-error mb-3 rounded-xl px-3 py-2 text-xs">{error}</div>
          ) : null}

          {(!isAdmin || activeInbox === 'user') && (
            <>
              {loading && invites.length === 0 ? (
                <div className="panel-soft rounded-xl px-3 py-3 text-sm muted">Loading notifications...</div>
              ) : invites.length === 0 ? (
                <div className="panel-soft rounded-xl px-3 py-3 text-sm muted">
                  No user notifications right now.
                </div>
              ) : (
                <ul className="max-h-[24rem] space-y-2 overflow-y-auto pr-1">
                  {invites.map((invite) => (
                    <li key={invite.room_id} className="tile rounded-xl px-3 py-2.5">
                      <div className="flex items-start justify-between gap-3">
                        <div className="min-w-0">
                          <p className="truncate text-sm font-medium">{invite.item_title}</p>
                          <p className="text-xs muted">
                            Host: {invite.host_username} - Role: {roleLabel(invite.role, 'video')}
                            {invite.password_required ? ' - Password required' : ''}
                          </p>
                        </div>
                        <span
                          className="chip shrink-0 text-[10px]"
                          title={formatAbsoluteTime(invite.created_ts)}
                        >
                          {formatRelativeTime(invite.created_ts)}
                        </span>
                      </div>
                      <div className="mt-2 flex items-center gap-2">
                        <Link
                          href={`/rooms/${invite.room_id}`}
                          className="btn-secondary px-3 py-1.5 text-xs"
                          onClick={() => setOpen(false)}
                        >
                          Open room
                        </Link>
                        <button
                          type="button"
                          className="btn-ghost px-3 py-1.5 text-xs"
                          onClick={() => void handleDeclineInvite(invite.room_id)}
                          disabled={decliningRoomId === invite.room_id}
                        >
                          {decliningRoomId === invite.room_id ? 'Declining...' : 'Decline'}
                        </button>
                      </div>
                    </li>
                  ))}
                </ul>
              )}
            </>
          )}

          {isAdmin && activeInbox === 'admin' && (
            <>
              {loading && adminNotifications.length === 0 ? (
                <div className="panel-soft rounded-xl px-3 py-3 text-sm muted">Loading admin notifications...</div>
              ) : adminNotifications.length === 0 ? (
                <div className="panel-soft rounded-xl px-3 py-3 text-sm muted">
                  No admin diagnostics yet.
                </div>
              ) : (
                <ul className="max-h-[24rem] space-y-2 overflow-y-auto pr-1">
                  {adminNotifications.map((notification) => (
                    <li key={notification.id} className="tile rounded-xl px-3 py-2.5">
                      <div className="flex items-start justify-between gap-3">
                        <div className="min-w-0">
                          <p className="truncate text-sm font-medium">{notification.title}</p>
                          <p className="truncate text-xs muted">{notification.detail}</p>
                        </div>
                        <span className="chip shrink-0 text-[10px]">{kindLabel(notification.kind)}</span>
                      </div>
                      <div className="mt-2 flex items-center justify-between gap-3">
                        <p
                          className="text-[11px] muted"
                          title={formatAbsoluteTime(notification.timestamp_ts)}
                        >
                          {formatRelativeTime(notification.timestamp_ts)}
                        </p>
                        <Link
                          href="/admin"
                          className="btn-ghost px-2.5 py-1 text-xs"
                          onClick={() => setOpen(false)}
                        >
                          Open admin
                        </Link>
                      </div>
                    </li>
                  ))}
                </ul>
              )}
            </>
          )}
        </div>
      ) : null}
    </div>
  );
}
