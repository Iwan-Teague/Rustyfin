'use client';

import { useEffect, useRef, useState, type ReactNode } from 'react';
import type { ChannelInfo, UserInfo } from '@/lib/channelsApi';
import { renameChannel } from '@/lib/channelsApi';
import { elapsedSinceSeconds, formatElapsedSeconds } from '@/lib/time';
import { findDataDeleteTarget, playTelegramDeleteAnimation } from '@/lib/deleteAnimation';
import { useListReflowAnimation } from '@/lib/listReflowAnimation';
import ConfirmModal from '@/app/components/ConfirmModal';

const DELETE_AFTER_CONFIRM_DELAY_MS = 500;

interface Props {
  channels: ChannelInfo[];
  voicePresence: Record<string, UserInfo[]>;
  voiceActiveSince: Record<string, number>;
  voiceSpeaking: Record<string, string[]>;
  activeChannelId: string | null;
  connectedVoiceChannelId: string | null;
  isAdmin: boolean;
  onSelect: (id: string) => void;
  onQuickJoinVoice: (id: string, name: string) => void;
  onCreateText: () => void;
  onCreateVoice: () => void;
  onDeleteChannel: (id: string) => void;
  bottomContent?: ReactNode;
}

// ── Channel context menu ──────────────────────────────────────────────────────

interface MenuState {
  channelId: string;
  channelName: string;
}

interface ContextMenuProps {
  channel: ChannelInfo;
  onClose: () => void;
  onRequestRename: (channel: ChannelInfo) => void;
  onRequestDelete: () => void;
  onCannotDelete: (channel: ChannelInfo, membersInChannel: number) => void;
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

function ChannelContextMenu({
  channel,
  onClose,
  onRequestRename,
  onRequestDelete,
  onCannotDelete,
  membersInChannel = 0,
}: ContextMenuProps) {
  const menuRef = useRef<HTMLDivElement>(null);

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

  return (
    <div
      ref={menuRef}
      className="absolute right-0 top-full mt-1 z-50 panel rounded-xl shadow-xl border border-[var(--border)] w-44 py-1 text-sm"
      onClick={(e) => e.stopPropagation()}
    >
      <button
        className="w-full text-left px-3 py-2 hover:bg-white/5 rounded-md"
        onClick={() => {
          onRequestRename(channel);
          onClose();
        }}
      >
        Rename
      </button>
      <button
        className="w-full text-left px-3 py-2 hover:bg-white/5 rounded-md text-red-400 hover:text-red-300"
        onClick={() => {
          if (channel.kind === 'voice' && membersInChannel > 0) {
            onCannotDelete(channel, membersInChannel);
          } else {
            onRequestDelete();
          }
          onClose();
        }}
      >
        Delete
      </button>
    </div>
  );
}

// ── Channel row ───────────────────────────────────────────────────────────────

interface ChannelRowProps {
  ch: ChannelInfo;
  icon: ReactNode;
  voicePresence: Record<string, UserInfo[]>;
  voiceActiveSince: Record<string, number>;
  voiceSpeaking: Record<string, string[]>;
  nowMs: number;
  isAdmin: boolean;
  activeChannelId: string | null;
  connectedVoiceChannelId: string | null;
  menuOpen: MenuState | null;
  setMenuOpen: (state: MenuState | null) => void;
  onSelect: (id: string) => void;
  onQuickJoinVoice: (id: string, name: string) => void;
  onRequestRenameChannel: (channel: ChannelInfo) => void;
  onRequestDeleteChannel: (channel: ChannelInfo) => void;
  onCannotDeleteChannel: (channel: ChannelInfo, membersInChannel: number) => void;
}

function ChannelRow({
  ch,
  icon,
  voicePresence,
  voiceActiveSince,
  voiceSpeaking,
  nowMs,
  isAdmin,
  activeChannelId,
  connectedVoiceChannelId,
  menuOpen,
  setMenuOpen,
  onSelect,
  onQuickJoinVoice,
  onRequestRenameChannel,
  onRequestDeleteChannel,
  onCannotDeleteChannel,
}: ChannelRowProps) {
  const members = voicePresence[ch.id] ?? [];
  const speakingIds = new Set(voiceSpeaking[ch.id] ?? []);
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
    <div data-list-item-id={ch.id} data-channel-row-id={ch.id}>
      <div
        className={rowClass}
        onClick={() => onSelect(ch.id)}
        onDoubleClick={() => {
          if (ch.kind !== 'voice') return;
          if (connectedVoiceChannelId === ch.id) return;
          onQuickJoinVoice(ch.id, ch.name);
        }}
        onTouchEnd={() => {
          if (ch.kind !== 'voice') return;
          const now = Date.now();
          if (now - lastTapAtRef.current < 320) {
            if (connectedVoiceChannelId === ch.id) return;
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
              className="btn-ghost px-1 py-0.5 text-xs opacity-60 md:opacity-0 md:group-hover:opacity-60 hover:!opacity-100 transition-opacity"
              title="Channel options"
            >
              ⋯
            </button>
            {isMenuOpen && (
              <ChannelContextMenu
                channel={ch}
                onClose={() => setMenuOpen(null)}
                onRequestRename={onRequestRenameChannel}
                onRequestDelete={() => onRequestDeleteChannel(ch)}
                onCannotDelete={onCannotDeleteChannel}
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
            className="inline-flex rounded-full p-[2px] transition-all duration-150 shrink-0"
            style={
              speakingIds.has(u.user_id)
                ? {
                    background:
                      'linear-gradient(115deg, var(--orange) 0%, var(--purple-strong) 75%)',
                    boxShadow: '0 0 0 1px rgba(255, 145, 77, 0.35)',
                  }
                : undefined
            }
            aria-hidden="true"
          >
            {u.avatar_url ? (
              <img
                src={u.avatar_url}
                alt={u.username}
                className="inline-flex h-5 w-5 rounded-full object-cover border border-[var(--border)] bg-black/20"
                loading="lazy"
              />
            ) : (
              <span
                className="inline-flex h-5 w-5 items-center justify-center rounded-full text-[10px] font-semibold text-white"
                style={{ backgroundColor: userBubbleColor(u.user_id) }}
              >
                {u.username.slice(0, 2).toUpperCase()}
              </span>
            )}
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
  voiceSpeaking,
  activeChannelId,
  connectedVoiceChannelId,
  isAdmin,
  onSelect,
  onQuickJoinVoice,
  onCreateText,
  onCreateVoice,
  onDeleteChannel,
  bottomContent,
}: Props) {
  const [menuOpen, setMenuOpen] = useState<MenuState | null>(null);
  const [pendingRenameChannel, setPendingRenameChannel] = useState<ChannelInfo | null>(null);
  const [renameValue, setRenameValue] = useState('');
  const [renameError, setRenameError] = useState('');
  const [pendingDeleteChannel, setPendingDeleteChannel] = useState<ChannelInfo | null>(null);
  const [cannotDeleteChannel, setCannotDeleteChannel] = useState<{
    name: string;
    membersInChannel: number;
  } | null>(null);
  const [nowMs, setNowMs] = useState(() => Date.now());
  const channelListRef = useRef<HTMLDivElement>(null);
  const renameInputRef = useRef<HTMLInputElement>(null);

  const textChannels = channels.filter((c) => c.kind === 'text');
  const voiceChannels = channels.filter((c) => c.kind === 'voice');
  const hasActiveVoice = Object.keys(voiceActiveSince).length > 0;

  useEffect(() => {
    if (!hasActiveVoice) return;
    const id = window.setInterval(() => setNowMs(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [hasActiveVoice]);

  useListReflowAnimation(channelListRef, channels.map((channel) => channel.id), {
    itemSelector: '[data-list-item-id]',
  });

  useEffect(() => {
    if (!pendingRenameChannel) return;
    setRenameValue(pendingRenameChannel.name);
    setRenameError('');
    window.setTimeout(() => renameInputRef.current?.focus(), 0);
  }, [pendingRenameChannel]);

  const handleRenameChannel = async () => {
    if (!pendingRenameChannel) return;
    const nextName = renameValue.trim();
    if (!nextName) {
      setRenameError('Name cannot be empty');
      return;
    }
    if (nextName === pendingRenameChannel.name) {
      setPendingRenameChannel(null);
      return;
    }
    setRenameError('');
    try {
      await renameChannel(pendingRenameChannel.id, nextName);
      setPendingRenameChannel(null);
    } catch {
      setRenameError('Failed to rename channel');
    }
  };

  return (
    <aside className="flex flex-col w-60 min-w-[200px] bg-[var(--surface)] border-r border-[var(--border)] h-full overflow-hidden">
      {/* Server header */}
      <div className="h-14 px-4 border-b border-[var(--border)] font-semibold text-sm tracking-wide flex items-center">
        Rustyfin
      </div>

      <div ref={channelListRef} className="flex-1 overflow-y-auto py-2 space-y-4">
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
              voiceSpeaking={voiceSpeaking}
              nowMs={nowMs}
              isAdmin={isAdmin}
              activeChannelId={activeChannelId}
              connectedVoiceChannelId={connectedVoiceChannelId}
              menuOpen={menuOpen}
              setMenuOpen={setMenuOpen}
              onSelect={onSelect}
              onQuickJoinVoice={onQuickJoinVoice}
              onRequestRenameChannel={setPendingRenameChannel}
              onRequestDeleteChannel={setPendingDeleteChannel}
              onCannotDeleteChannel={(channel, membersInChannel) =>
                setCannotDeleteChannel({ name: channel.name, membersInChannel })
              }
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
              voiceSpeaking={voiceSpeaking}
              nowMs={nowMs}
              isAdmin={isAdmin}
              activeChannelId={activeChannelId}
              connectedVoiceChannelId={connectedVoiceChannelId}
              menuOpen={menuOpen}
              setMenuOpen={setMenuOpen}
              onSelect={onSelect}
              onQuickJoinVoice={onQuickJoinVoice}
              onRequestRenameChannel={setPendingRenameChannel}
              onRequestDeleteChannel={setPendingDeleteChannel}
              onCannotDeleteChannel={(channel, membersInChannel) =>
                setCannotDeleteChannel({ name: channel.name, membersInChannel })
              }
            />
          ))}

          {voiceChannels.length === 0 && (
            <p className="px-3 py-1 text-xs muted italic">No voice channels yet</p>
          )}
        </section>
      </div>

      {bottomContent}

      {pendingRenameChannel && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/45 backdrop-blur-[2px] p-4">
          <div className="panel rounded-2xl p-6 w-full max-w-sm space-y-4 border border-[var(--border)]">
            <h2 className="font-semibold text-lg">Rename Channel</h2>
            <p className="text-sm muted">
              Enter a new name for &ldquo;{pendingRenameChannel.name}&rdquo;.
            </p>
            <input
              ref={renameInputRef}
              className="input rounded-xl px-3 py-2 text-sm"
              value={renameValue}
              onChange={(e) => setRenameValue(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') {
                  void handleRenameChannel();
                }
                if (e.key === 'Escape') {
                  setPendingRenameChannel(null);
                }
              }}
              maxLength={64}
            />
            {renameError ? <p className="text-xs text-red-400">{renameError}</p> : null}
            <div className="flex gap-2 justify-end">
              <button
                onClick={() => setPendingRenameChannel(null)}
                className="btn-ghost px-4 py-2 text-sm"
              >
                Cancel
              </button>
              <button onClick={() => void handleRenameChannel()} className="btn-primary px-4 py-2 text-sm">
                Save
              </button>
            </div>
          </div>
        </div>
      )}

      <ConfirmModal
        open={Boolean(cannotDeleteChannel)}
        title="Cannot Delete Channel"
        description={
          cannotDeleteChannel ? (
            <>
              &ldquo;{cannotDeleteChannel.name}&rdquo; still has {cannotDeleteChannel.membersInChannel}{' '}
              {cannotDeleteChannel.membersInChannel === 1 ? 'member' : 'members'} connected. Ask everyone to leave first.
            </>
          ) : undefined
        }
        confirmLabel="Okay"
        hideCancel
        onCancel={() => setCannotDeleteChannel(null)}
        onConfirm={() => setCannotDeleteChannel(null)}
      />

      <ConfirmModal
        open={Boolean(pendingDeleteChannel)}
        title="Delete Channel"
        description={
          pendingDeleteChannel ? (
            <>
              Delete &ldquo;{pendingDeleteChannel.name}&rdquo;? All messages will be lost and this cannot be undone.
            </>
          ) : undefined
        }
        confirmLabel="Delete"
        destructive
        onCancel={() => setPendingDeleteChannel(null)}
        onConfirm={() => {
          void (async () => {
            const channelToDelete = pendingDeleteChannel;
            if (!channelToDelete) return;
            setPendingDeleteChannel(null);
            if (menuOpen?.channelId === channelToDelete.id) {
              setMenuOpen(null);
            }
            await new Promise<void>((resolve) => {
              window.setTimeout(resolve, DELETE_AFTER_CONFIRM_DELAY_MS);
            });
            const target = findDataDeleteTarget('data-channel-row-id', channelToDelete.id);
            await playTelegramDeleteAnimation(target);
            onDeleteChannel(channelToDelete.id);
          })();
        }}
      />
    </aside>
  );
}
