'use client';

import { useEffect, useRef, useState, type ReactNode } from 'react';
import type { ChannelInfo, UserInfo } from '@/lib/channelsApi';
import { renameChannel } from '@/lib/channelsApi';
import { elapsedSinceSeconds, formatElapsedSeconds } from '@/lib/time';

interface Props {
  channels: ChannelInfo[];
  voicePresence: Record<string, UserInfo[]>;
  voiceActiveSince: Record<string, number>;
  activeChannelId: string | null;
  isAdmin: boolean;
  onSelect: (id: string) => void;
  onQuickJoinVoice: (id: string, name: string) => void;
  onCreateText: () => void;
  onCreateVoice: () => void;
  onDeleteChannel: (id: string) => void;
}

// ── Channel context menu ──────────────────────────────────────────────────────

interface MenuState {
  channelId: string;
  channelName: string;
}

interface ContextMenuProps {
  channel: ChannelInfo;
  onClose: () => void;
  onDelete: () => void;
  membersInChannel?: number;
}

function userBubbleColor(userId: string): string {
  let hash = 0;
  for (let i = 0; i < userId.length; i++) {
    hash = userId.charCodeAt(i) + ((hash << 5) - hash);
  }
  const colors = [
    '#e67e22', '#3498db', '#2ecc71', '#9b59b6', '#e74c3c',
    '#1abc9c', '#f39c12', '#16a085', '#d35400', '#8e44ad',
  ];
  return colors[Math.abs(hash) % colors.length];
}

function ChannelContextMenu({ channel, onClose, onDelete, membersInChannel = 0 }: ContextMenuProps) {
  const [view, setView] = useState<'menu' | 'rename' | 'confirm' | 'error'>('menu');
  const [renameValue, setRenameValue] = useState(channel.name);
  const [renameError, setRenameError] = useState('');
  const menuRef = useRef<HTMLDivElement>(null);
  const renameRef = useRef<HTMLInputElement>(null);

  // Close on outside click
  useEffect(() => {
    function handleClick(e: MouseEvent) {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        onClose();
      }
    }
    document.addEventListener('mousedown', handleClick);
    return () => document.removeEventListener('mousedown', handleClick);
  }, [onClose]);

  // Focus rename input when view switches
  useEffect(() => {
    if (view === 'rename') renameRef.current?.focus();
  }, [view]);

  async function handleRename() {
    const name = renameValue.trim();
    if (!name) { setRenameError('Name cannot be empty'); return; }
    if (name === channel.name) { onClose(); return; }
    setRenameError('');
    try {
      await renameChannel(channel.id, name);
      onClose();
    } catch {
      setRenameError('Failed to rename channel');
    }
  }

  return (
    <div
      ref={menuRef}
      className="absolute right-0 top-full mt-1 z-50 panel rounded-xl shadow-xl border border-[var(--border)] w-44 py-1 text-sm"
      onClick={(e) => e.stopPropagation()}
    >
      {view === 'menu' && (
        <>
          <button
            className="w-full text-left px-3 py-2 hover:bg-white/5 rounded-md"
            onClick={() => setView('rename')}
          >
            Rename
          </button>
          <button
            className="w-full text-left px-3 py-2 hover:bg-white/5 rounded-md text-red-400 hover:text-red-300"
            onClick={() => {
              if (channel.kind === 'voice' && membersInChannel > 0) {
                setView('error');
              } else {
                setView('confirm');
              }
            }}
          >
            Delete
          </button>
        </>
      )}

      {view === 'rename' && (
        <div className="px-3 py-2 space-y-2">
          <p className="text-xs muted font-semibold uppercase tracking-wide">Rename channel</p>
          <input
            ref={renameRef}
            className="panel w-full rounded-md px-2 py-1 text-sm"
            value={renameValue}
            onChange={(e) => setRenameValue(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') handleRename();
              if (e.key === 'Escape') onClose();
            }}
            maxLength={64}
          />
          {renameError && <p className="text-xs text-red-400">{renameError}</p>}
          <div className="flex gap-1 justify-end">
            <button onClick={onClose} className="btn-ghost px-2 py-1 text-xs">Cancel</button>
            <button onClick={handleRename} className="btn-primary px-2 py-1 text-xs">Save</button>
          </div>
        </div>
      )}

      {view === 'confirm' && (
        <div className="px-3 py-2 space-y-2">
          <p className="text-xs font-semibold">Delete &ldquo;{channel.name}&rdquo;?</p>
          <p className="text-xs muted">All messages will be lost. This cannot be undone.</p>
          <div className="flex gap-1 justify-end">
            <button onClick={onClose} className="btn-ghost px-2 py-1 text-xs">Cancel</button>
            <button
              onClick={() => { onDelete(); onClose(); }}
              className="btn-primary px-2 py-1 text-xs bg-red-500 hover:bg-red-600"
            >
              Delete
            </button>
          </div>
        </div>
      )}

      {view === 'error' && (
        <div className="px-3 py-2 space-y-2">
          <p className="text-xs font-semibold text-red-400">Cannot Delete Channel</p>
          <p className="text-xs muted">This audio channel needs to be empty before deletion. Ask members to leave first.</p>
          <div className="flex gap-1 justify-end">
            <button onClick={() => setView('menu')} className="btn-ghost px-2 py-1 text-xs">Back</button>
          </div>
        </div>
      )}
    </div>
  );
}

// ── Channel row ───────────────────────────────────────────────────────────────

interface ChannelRowProps {
  ch: ChannelInfo;
  icon: ReactNode;
  voicePresence: Record<string, UserInfo[]>;
  voiceActiveSince: Record<string, number>;
  nowMs: number;
  isAdmin: boolean;
  activeChannelId: string | null;
  menuOpen: MenuState | null;
  setMenuOpen: (state: MenuState | null) => void;
  onSelect: (id: string) => void;
  onQuickJoinVoice: (id: string, name: string) => void;
  onDeleteChannel: (id: string) => void;
}

function ChannelRow({
  ch,
  icon,
  voicePresence,
  voiceActiveSince,
  nowMs,
  isAdmin,
  activeChannelId,
  menuOpen,
  setMenuOpen,
  onSelect,
  onQuickJoinVoice,
  onDeleteChannel,
}: ChannelRowProps) {
  const members = voicePresence[ch.id] ?? [];
  const activeSinceTs = voiceActiveSince[ch.id];
  const isMenuOpen = menuOpen?.channelId === ch.id;
  const lastTapAtRef = useRef(0);

  const isActive = ch.id === activeChannelId;
  const rowClass = [
    'flex items-center gap-2 px-2 py-1.5 rounded-md cursor-pointer text-sm group relative select-none',
    isActive
      ? 'border-l-2 border-[var(--orange-soft)] bg-white/5 pl-1.5'
      : 'hover:bg-white/5',
  ].join(' ');

  return (
    <div>
      <div
        className={rowClass}
        onClick={() => onSelect(ch.id)}
        onDoubleClick={() => {
          if (ch.kind !== 'voice') return;
          onQuickJoinVoice(ch.id, ch.name);
        }}
        onTouchEnd={() => {
          if (ch.kind !== 'voice') return;
          const now = Date.now();
          if (now - lastTapAtRef.current < 320) {
            onQuickJoinVoice(ch.id, ch.name);
          }
          lastTapAtRef.current = now;
        }}
      >
        <span className="muted shrink-0">{icon}</span>
        <span className="truncate flex-1">{ch.name}</span>
        {ch.kind === 'voice' && members.length > 0 && (
          <span className="text-xs shrink-0 muted inline-flex items-center">
            <span className="inline-flex h-5 w-5 items-center justify-center rounded-full border border-[var(--border)] bg-black/20 text-[10px] font-semibold">
              {members.length}
            </span>
          </span>
        )}
        {isAdmin && (
          <div className="relative shrink-0">
            <button
              onClick={(e) => {
                e.stopPropagation();
                setMenuOpen(isMenuOpen ? null : { channelId: ch.id, channelName: ch.name });
              }}
              className="btn-ghost px-1 py-0.5 text-xs opacity-0 group-hover:opacity-60 hover:!opacity-100 transition-opacity"
              title="Channel options"
            >
              ⋯
            </button>
            {isMenuOpen && (
              <ChannelContextMenu
                channel={ch}
                onClose={() => setMenuOpen(null)}
                onDelete={() => onDeleteChannel(ch.id)}
                membersInChannel={members.length}
              />
            )}
          </div>
        )}
      </div>
      {ch.kind === 'voice' && members.length > 0 && activeSinceTs !== undefined && (
        <div className="pl-8 py-0.5 text-[11px] muted">
          Live for {formatElapsedSeconds(elapsedSinceSeconds(activeSinceTs, nowMs))}
        </div>
      )}
      {/* Voice member rows */}
      {ch.kind === 'voice' && members.map((u) => (
        <div key={u.user_id} className="pl-8 py-0.5 text-xs muted flex items-center gap-2">
          <span
            className="inline-flex h-5 w-5 items-center justify-center rounded-full text-[10px] font-semibold text-white shrink-0"
            style={{ backgroundColor: userBubbleColor(u.user_id) }}
            aria-hidden="true"
          >
            {u.username.slice(0, 2).toUpperCase()}
          </span>
          <span className="truncate">{u.username}</span>
        </div>
      ))}
    </div>
  );
}

// ── Sidebar ───────────────────────────────────────────────────────────────────

export default function ChannelSidebar({
  channels,
  voicePresence,
  voiceActiveSince,
  activeChannelId,
  isAdmin,
  onSelect,
  onQuickJoinVoice,
  onCreateText,
  onCreateVoice,
  onDeleteChannel,
}: Props) {
  const [menuOpen, setMenuOpen] = useState<MenuState | null>(null);
  const [nowMs, setNowMs] = useState(() => Date.now());

  const textChannels = channels.filter((c) => c.kind === 'text');
  const voiceChannels = channels.filter((c) => c.kind === 'voice');
  const hasActiveVoice = Object.keys(voiceActiveSince).length > 0;

  useEffect(() => {
    if (!hasActiveVoice) return;
    const id = window.setInterval(() => setNowMs(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [hasActiveVoice]);

  return (
    <aside className="flex flex-col w-60 min-w-[200px] bg-[var(--surface)] border-r border-[var(--border)] h-full overflow-y-auto">
      {/* Server header */}
      <div className="h-14 px-4 border-b border-[var(--border)] font-semibold text-sm tracking-wide flex items-center">
        Rustyfin
      </div>

      <div className="flex-1 overflow-y-auto py-2 space-y-4">
        {/* TEXT CHANNELS */}
        <section>
          <div className="flex items-center justify-between px-3 py-1">
            <span className="text-xs font-semibold muted uppercase tracking-wider">
              Text Channels
            </span>
            {isAdmin && (
              <button
                onClick={onCreateText}
                className="btn-ghost px-1 py-0.5 text-lg leading-none"
                title="Create text channel"
              >
                +
              </button>
            )}
          </div>

          {textChannels.map((ch) => (
            <ChannelRow
              key={ch.id}
              ch={ch}
              icon="#"
              voicePresence={voicePresence}
              voiceActiveSince={voiceActiveSince}
              nowMs={nowMs}
              isAdmin={isAdmin}
              activeChannelId={activeChannelId}
              menuOpen={menuOpen}
              setMenuOpen={setMenuOpen}
              onSelect={onSelect}
              onQuickJoinVoice={onQuickJoinVoice}
              onDeleteChannel={onDeleteChannel}
            />
          ))}

          {textChannels.length === 0 && (
            <p className="px-3 py-1 text-xs muted italic">No text channels yet</p>
          )}
        </section>

        {/* VOICE CHANNELS */}
        <section>
          <div className="flex items-center justify-between px-3 py-1">
            <span className="text-xs font-semibold muted uppercase tracking-wider">
              Voice Channels
            </span>
            {isAdmin && (
              <button
                onClick={onCreateVoice}
                className="btn-ghost px-1 py-0.5 text-lg leading-none"
                title="Create voice channel"
              >
                +
              </button>
            )}
          </div>

          {voiceChannels.map((ch) => (
            <ChannelRow
              key={ch.id}
              ch={ch}
              icon=""
              voicePresence={voicePresence}
              voiceActiveSince={voiceActiveSince}
              nowMs={nowMs}
              isAdmin={isAdmin}
              activeChannelId={activeChannelId}
              menuOpen={menuOpen}
              setMenuOpen={setMenuOpen}
              onSelect={onSelect}
              onQuickJoinVoice={onQuickJoinVoice}
              onDeleteChannel={onDeleteChannel}
            />
          ))}

          {voiceChannels.length === 0 && (
            <p className="px-3 py-1 text-xs muted italic">No voice channels yet</p>
          )}
        </section>
      </div>
    </aside>
  );
}
