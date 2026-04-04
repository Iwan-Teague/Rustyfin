'use client';

import { useEffect, useRef, useState } from 'react';
import { useRouter } from 'next/navigation';
import { useAuth } from '@/lib/auth';
import { useChannels } from '@/lib/channelsContext';
import { createChannel, deleteChannel, postMessage } from '@/lib/channelsApi';

import ChannelSidebar from './components/ChannelSidebar';
import TextChannelView from './components/TextChannelView';
import VoiceChannelView from './components/VoiceChannelView';
import ChannelUserSettings from './components/ChannelUserSettings';

const LAST_USED_CHANNEL_KEY = 'rustyfin:last-used-channel-id';

export default function ChannelsPage() {
  const { me, loading: authLoading } = useAuth();
  const router = useRouter();
  const {
    wsReady,
    channels,
    voicePresence,
    voiceActiveSince,
    voiceSpeaking,
    connectedVoiceChannelId,
    preferredInputDeviceId,
    preferredOutputDeviceId,
    newMessages,
    lastWsEvent,
    joinVoice,
    setPreferredAudioDevices,
  } = useChannels();

  const [activeChannelId, setActiveChannelId] = useState<string | null>(null);
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [desktopSidebarOpen, setDesktopSidebarOpen] = useState(true);
  const [isDesktop, setIsDesktop] = useState(false);
  const preselectedApplied = useRef(false);

  // Auto-select a channel when deep-linking via ?channel=<id>, last-used choice, or default channel
  useEffect(() => {
    if (preselectedApplied.current || activeChannelId || channels.length === 0) return;
    const params = new URLSearchParams(window.location.search);
    const channelId = params.get('channel');
    const lastUsedChannelId =
      typeof window !== 'undefined' ? window.localStorage.getItem(LAST_USED_CHANNEL_KEY) : null;
    const firstTextChannel = channels.find((channel) => channel.kind === 'text') ?? null;
    const firstVoiceChannel = channels.find((channel) => channel.kind === 'voice') ?? null;
    const fallbackChannelId =
      lastUsedChannelId && channels.some((channel) => channel.id === lastUsedChannelId)
        ? lastUsedChannelId
        : connectedVoiceChannelId && channels.some((channel) => channel.id === connectedVoiceChannelId)
          ? connectedVoiceChannelId
          : firstTextChannel?.id ?? firstVoiceChannel?.id ?? channels[0]?.id ?? null;

    if (channelId && channels.some((c) => c.id === channelId)) {
      preselectedApplied.current = true;
      setActiveChannelId(channelId);
    } else if (fallbackChannelId) {
      preselectedApplied.current = true;
      setActiveChannelId(fallbackChannelId);
    }
  }, [channels, activeChannelId, connectedVoiceChannelId]);

  useEffect(() => {
    if (!activeChannelId || typeof window === 'undefined') return;
    window.localStorage.setItem(LAST_USED_CHANNEL_KEY, activeChannelId);
  }, [activeChannelId]);

  useEffect(() => {
    if (!activeChannelId) return;
    if (channels.some((channel) => channel.id === activeChannelId)) return;
    setActiveChannelId(null);
    preselectedApplied.current = false;
  }, [activeChannelId, channels]);

  // Modal state
  const [createModal, setCreateModal] = useState<{ kind: 'text' | 'voice' } | null>(null);
  const [createName, setCreateName] = useState('');
  const [createPrivate, setCreatePrivate] = useState(false);
  const [createError, setCreateError] = useState('');
  const createDialogTitleId = 'create-channel-dialog-title';
  const createDialogFieldId = 'create-channel-name';

  useEffect(() => {
    document.documentElement.dataset.rfPage = 'channels';
    document.body.dataset.rfPage = 'channels';
    return () => {
      delete document.documentElement.dataset.rfPage;
      delete document.body.dataset.rfPage;
    };
  }, []);

  useEffect(() => {
    if (typeof window === 'undefined') return;
    const media = window.matchMedia('(min-width: 768px)');
    const update = () => setIsDesktop(media.matches);
    update();
    media.addEventListener('change', update);
    return () => media.removeEventListener('change', update);
  }, []);

  if (!authLoading && !me) {
    router.replace('/login');
    return null;
  }

  if (authLoading || !me) {
    return (
      <div className="flex items-center justify-center h-full py-20 animate-rise">
        <span className="muted">Loading…</span>
      </div>
    );
  }

  const activeChannel = channels.find((c) => c.id === activeChannelId) ?? null;

  const handleSendMessage = async (content: string) => {
    if (!activeChannelId) return null;
    return postMessage(activeChannelId, content);
  };

  const handleCreateChannel = async () => {
    if (!createModal) return;
    const name = createName.trim();
    if (!name) { setCreateError('Name is required'); return; }
    setCreateError('');
    try {
      await createChannel({ name, kind: createModal.kind, is_private: createPrivate });
      setCreateModal(null);
      setCreateName('');
      setCreatePrivate(false);
    } catch {
      setCreateError('Failed to create channel');
    }
  };

  const handleDeleteChannel = async (id: string) => {
    try {
      await deleteChannel(id);
    } catch {
      // Deletion errors are non-fatal — the server still processes the delete
      // via the WebSocket broadcast, so the channel will disappear regardless.
    }
  };

  const handleToggleSidebar = () => {
    if (typeof window !== 'undefined' && window.innerWidth >= 768) {
      setDesktopSidebarOpen((current) => !current);
      return;
    }
    setSidebarOpen((current) => !current);
  };

  const desktopSidebarWidth = desktopSidebarOpen ? '15rem' : '0px';

  return (
    <div className="animate-rise flex h-full min-h-0 w-full overflow-hidden">
      {/* Sidebar */}
      <aside
        className="ai-side-panel-shell ai-side-panel-shell-left hidden md:flex md:min-h-0 md:flex-col md:overflow-hidden"
        data-open={desktopSidebarOpen ? 'true' : 'false'}
        data-side="left"
        style={{ width: desktopSidebarWidth }}
      >
        <div className="ai-side-panel-inner flex h-full min-h-0 flex-col border-r border-[var(--border)]">
          <ChannelSidebar
            channels={channels}
            voicePresence={voicePresence}
            voiceActiveSince={voiceActiveSince}
            voiceSpeaking={voiceSpeaking}
            activeChannelId={activeChannelId}
            connectedVoiceChannelId={connectedVoiceChannelId}
            isAdmin={me.role === 'admin'}
            onSelect={(id) => { setActiveChannelId(id); setSidebarOpen(false); }}
            onQuickJoinVoice={(id, name) => {
              void joinVoice(id, name);
            }}
            onCreateText={() => { setCreateModal({ kind: 'text' }); setCreateName(''); setCreatePrivate(false); setCreateError(''); }}
            onCreateVoice={() => { setCreateModal({ kind: 'voice' }); setCreateName(''); setCreatePrivate(false); setCreateError(''); }}
            onDeleteChannel={handleDeleteChannel}
            bottomContent={
              <ChannelUserSettings
                me={me}
                preferredInputDeviceId={preferredInputDeviceId}
                preferredOutputDeviceId={preferredOutputDeviceId}
                setPreferredAudioDevices={setPreferredAudioDevices}
              />
            }
          />
        </div>
      </aside>

      <div className={['fixed inset-y-0 left-0 z-40 w-60 max-w-[85vw] md:hidden', sidebarOpen ? 'block' : 'hidden'].join(' ')}>
        <div className="absolute inset-0 bg-black/50" onClick={() => setSidebarOpen(false)} />
        <div className="relative h-full w-full border-r border-[var(--border)] bg-[var(--surface)]">
          <ChannelSidebar
            channels={channels}
            voicePresence={voicePresence}
            voiceActiveSince={voiceActiveSince}
            voiceSpeaking={voiceSpeaking}
            activeChannelId={activeChannelId}
            connectedVoiceChannelId={connectedVoiceChannelId}
            isAdmin={me.role === 'admin'}
            onSelect={(id) => { setActiveChannelId(id); setSidebarOpen(false); }}
            onQuickJoinVoice={(id, name) => {
              void joinVoice(id, name);
            }}
            onCreateText={() => { setCreateModal({ kind: 'text' }); setCreateName(''); setCreatePrivate(false); setCreateError(''); }}
            onCreateVoice={() => { setCreateModal({ kind: 'voice' }); setCreateName(''); setCreatePrivate(false); setCreateError(''); }}
            onDeleteChannel={handleDeleteChannel}
            bottomContent={
              <ChannelUserSettings
                me={me}
                preferredInputDeviceId={preferredInputDeviceId}
                preferredOutputDeviceId={preferredOutputDeviceId}
                setPreferredAudioDevices={setPreferredAudioDevices}
              />
            }
          />
        </div>
      </div>

      {/* Main content */}
      <div className="flex h-full min-h-0 flex-1 flex-col overflow-hidden bg-transparent">
        {!wsReady && (
          <div className="px-4 py-1 text-xs bg-yellow-900/30 text-yellow-300 text-center">
            Connecting…
          </div>
        )}

        <div className="flex min-h-0 flex-1 overflow-hidden">
          {!activeChannel ? (
            <div className="flex h-full items-center justify-center muted">
              <p className="text-sm">No channels are available yet.</p>
            </div>
          ) : activeChannel.kind === 'text' ? (
            <TextChannelView
              key={activeChannel.id}
              channel={activeChannel}
              newMessages={newMessages}
              currentUserId={me.id}
              isAdmin={me.role === 'admin'}
              wsEvents={lastWsEvent}
              onSendMessage={handleSendMessage}
              onToggleSidebar={handleToggleSidebar}
              sidebarVisible={isDesktop ? desktopSidebarOpen : sidebarOpen}
            />
          ) : (
            <VoiceChannelView
              key={activeChannel.id}
              channel={activeChannel}
              voicePresence={voicePresence}
              currentUserId={me.id}
              currentUsername={me.username}
              wsEvents={lastWsEvent}
              onToggleSidebar={handleToggleSidebar}
              sidebarVisible={isDesktop ? desktopSidebarOpen : sidebarOpen}
            />
          )}
        </div>

      </div>

      {/* Create channel modal */}
      {createModal && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
          onClick={() => setCreateModal(null)}
        >
          <div
            className="w-full max-w-sm space-y-4 rounded-2xl border border-[var(--border)] bg-[var(--surface)]/95 p-6"
            role="dialog"
            aria-modal="true"
            aria-labelledby={createDialogTitleId}
            onClick={(event) => event.stopPropagation()}
          >
            <h2 id={createDialogTitleId} className="font-semibold text-lg">
              Create {createModal.kind === 'text' ? 'Text' : 'Voice'} Channel
            </h2>
            <div className="space-y-2">
              <label htmlFor={createDialogFieldId} className="text-sm muted">
                Channel name
              </label>
              <input
                id={createDialogFieldId}
                className="rf-flat-input w-full rounded-xl px-3 py-2 text-sm"
                placeholder={createModal.kind === 'text' ? 'general' : 'Lobby'}
                value={createName}
                onChange={(e) => setCreateName(e.target.value)}
                onKeyDown={(e) => e.key === 'Enter' && handleCreateChannel()}
                autoFocus
              />
            </div>
            {me.role === 'admin' && (
              <label className="flex cursor-pointer items-center gap-2 text-sm">
                <input
                  type="checkbox"
                  checked={createPrivate}
                  onChange={(e) => setCreatePrivate(e.target.checked)}
                />
                Private channel (admins only)
              </label>
            )}
            {createError && <p className="text-sm text-red-400" role="alert">{createError}</p>}
            <div className="flex justify-end gap-2">
              <button onClick={() => setCreateModal(null)} className="btn-ghost px-4 py-2 text-sm">
                Cancel
              </button>
              <button onClick={handleCreateChannel} className="btn-primary px-4 py-2 text-sm">
                Create
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
