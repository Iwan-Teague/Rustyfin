'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import Link from 'next/link';
import { createPortal } from 'react-dom';
import { usePathname } from 'next/navigation';
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

type FloatingPanelPosition = {
  top: number;
  left: number;
  width: number;
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
  const pathname = usePathname();
  const rootRef = useRef<HTMLDivElement | null>(null);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const panelRef = useRef<HTMLDivElement | null>(null);
  const [open, setOpen] = useState(false);
  const [activeInbox, setActiveInbox] = useState<NotificationInbox>('user');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [invites, setInvites] = useState<WatchPartyInvite[]>([]);
  const [adminNotifications, setAdminNotifications] = useState<AdminDiagnosticNotification[]>([]);
  const [decliningRoomId, setDecliningRoomId] = useState<string | null>(null);
  const [floatingPosition, setFloatingPosition] = useState<FloatingPanelPosition | null>(null);

  const totalCount = invites.length + (isAdmin ? adminNotifications.length : 0);
  const activeInboxCount = activeInbox === 'admin' ? adminNotifications.length : invites.length;
  const isAiRoute = pathname.startsWith('/ai');
  const panelLabel = isAdmin
    ? activeInbox === 'admin'
      ? 'Admin notifications'
      : 'User notifications'
    : 'Notifications';

  const updateFloatingPosition = useCallback(() => {
    if (typeof window === 'undefined' || !triggerRef.current) return;
    const rect = triggerRef.current.getBoundingClientRect();
    const viewportPadding = 10;
    const width = Math.min(496, Math.max(280, window.innerWidth - viewportPadding * 2));
    const left = Math.min(
      Math.max(viewportPadding, rect.right - width),
      Math.max(viewportPadding, window.innerWidth - width - viewportPadding),
    );
    const top = Math.max(viewportPadding, rect.bottom + 10);
    setFloatingPosition({ top, left, width });
  }, []);

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
    void refreshNotifications();
  }, [refreshNotifications]);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: MouseEvent) => {
      const target = event.target as Node | null;
      const inTrigger = Boolean(target && rootRef.current?.contains(target));
      const inPanel = Boolean(target && panelRef.current?.contains(target));
      if (target && !inTrigger && !inPanel) {
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
    updateFloatingPosition();
    const onLayoutChange = () => updateFloatingPosition();
    window.addEventListener('resize', onLayoutChange);
    window.addEventListener('scroll', onLayoutChange, true);
    const timer = window.setInterval(() => {
      void refreshNotifications();
    }, 45000);
    return () => {
      window.removeEventListener('resize', onLayoutChange);
      window.removeEventListener('scroll', onLayoutChange, true);
      window.clearInterval(timer);
    };
  }, [open, refreshNotifications, updateFloatingPosition]);

  const handleToggleOpen = useCallback(() => {
    if (open) {
      setOpen(false);
      return;
    }
    setActiveInbox('user');
    setFloatingPosition(null);
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

  const panel = open ? (
    <div
      ref={panelRef}
      role="dialog"
      aria-label={panelLabel}
      className="rf-overlay-enter rf-notify-popover rf-notify-popover-floating z-[95] rounded-2xl p-4"
      style={
        floatingPosition
          ? {
              top: floatingPosition.top,
              left: floatingPosition.left,
              width: floatingPosition.width,
              maxWidth: 'calc(100vw - 1.25rem)',
            }
          : { visibility: 'hidden' }
      }
    >
      <div className="rf-notify-header mb-3 flex items-start justify-between gap-3">
        <div className="space-y-1">
          <p className="rf-notify-kicker text-[11px] uppercase tracking-[0.16em] text-white/60">
            Inbox
          </p>
          <h2 className="text-base font-semibold leading-tight">
            Notifications <span className="text-sm muted">({activeInboxCount})</span>
          </h2>
          <p className="text-xs muted">
            {isAdmin
              ? 'User and admin inboxes. User opens by default.'
              : 'Invites and account updates.'}
          </p>
        </div>
        <button
          type="button"
          onClick={() => void refreshNotifications()}
          className="rf-notify-refresh btn-ghost px-3 py-1.5 text-xs"
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
          className="rf-notify-tabs mb-3"
          badgesClassName="rf-notify-tabs-badges"
          variant={isAiRoute ? 'surface' : 'flat'}
        />
      ) : null}

      {error ? (
        <div className="rf-notify-error notice-error mb-3 rounded-xl px-3 py-2 text-xs">{error}</div>
      ) : null}

      {(!isAdmin || activeInbox === 'user') && (
        <>
          {loading && invites.length === 0 ? (
            <div
              className={`rf-notify-empty ${isAiRoute ? 'panel-soft rounded-xl px-3 py-3' : 'rf-flat-empty'} text-sm muted`}
            >
              Loading notifications...
            </div>
          ) : invites.length === 0 ? (
            <div
              className={`rf-notify-empty ${isAiRoute ? 'panel-soft rounded-xl px-3 py-3' : 'rf-flat-empty'} text-sm muted`}
            >
              No user notifications right now.
            </div>
          ) : (
            <ul
              className={`rf-notify-list max-h-[24rem] overflow-y-auto pr-1 ${isAiRoute ? 'space-y-2' : 'rf-flat-list'}`}
            >
              {invites.map((invite) => (
                <li
                  key={invite.room_id}
                  className={`rf-notify-item ${isAiRoute ? 'tile rounded-xl px-3 py-2.5' : 'rf-flat-row'}`}
                >
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
                      className="rf-notify-item-action btn-secondary px-3 py-1.5 text-xs"
                      onClick={() => setOpen(false)}
                    >
                      Open room
                    </Link>
                    <button
                      type="button"
                      className="rf-notify-item-action btn-ghost px-3 py-1.5 text-xs"
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
            <div
              className={`rf-notify-empty ${isAiRoute ? 'panel-soft rounded-xl px-3 py-3' : 'rf-flat-empty'} text-sm muted`}
            >
              Loading admin notifications...
            </div>
          ) : adminNotifications.length === 0 ? (
            <div
              className={`rf-notify-empty ${isAiRoute ? 'panel-soft rounded-xl px-3 py-3' : 'rf-flat-empty'} text-sm muted`}
            >
              No admin diagnostics yet.
            </div>
          ) : (
            <ul
              className={`rf-notify-list max-h-[24rem] overflow-y-auto pr-1 ${isAiRoute ? 'space-y-2' : 'rf-flat-list'}`}
            >
              {adminNotifications.map((notification) => (
                <li
                  key={notification.id}
                  className={`rf-notify-item ${isAiRoute ? 'tile rounded-xl px-3 py-2.5' : 'rf-flat-row'}`}
                >
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
                      className="rf-notify-item-action btn-ghost px-2.5 py-1 text-xs"
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
  ) : null;

  return (
    <div ref={rootRef} className={`rf-notify-root relative shrink-0 ${className}`.trim()}>
      <button
        ref={triggerRef}
        type="button"
        className={`rf-notify-trigger rf-nav-link btn-ghost relative h-11 px-3 ${open ? 'is-open' : ''}`}
        aria-label="Open notifications"
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-current={open ? 'page' : undefined}
        onClick={handleToggleOpen}
      >
        <svg viewBox="0 0 24 24" className="rf-notify-bell h-5 w-5" fill="none" aria-hidden="true">
          <path
            d="M6 9a6 6 0 1 1 12 0v4l1.5 2.5H4.5L6 13V9Z"
            stroke="currentColor"
            strokeWidth="1.8"
            strokeLinejoin="round"
          />
          <path d="M10 18a2 2 0 0 0 4 0" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
        </svg>
        {totalCount > 0 ? (
          <span className="rf-notify-badge absolute -right-1 -top-1 z-[1] inline-flex h-5 min-w-5 items-center justify-center rounded-full px-1 text-[10px] font-semibold leading-none">
            {Math.min(totalCount, 99)}
          </span>
        ) : null}
      </button>

      {panel && typeof document !== 'undefined' ? createPortal(panel, document.body) : null}
    </div>
  );
}
