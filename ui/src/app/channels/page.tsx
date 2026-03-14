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
  const preselectedApplied = useRef(false);

  // Auto-select a channel when deep-linking via ?channel=<id>, or default to active voice channel
  useEffect(() => {
    if (preselectedApplied.current || activeChannelId || channels.length === 0) return;
    const params = new URLSearchParams(window.location.search);
    const channelId = params.get('channel');
    if (channelId && channels.some((c) => c.id === channelId)) {
      preselectedApplied.current = true;
      setActiveChannelId(channelId);
    } else if (
      connectedVoiceChannelId &&
      channels.some((c) => c.id === connectedVoiceChannelId)
    ) {
      // If no URL param, default to active voice channel if user is connected
      preselectedApplied.current = true;
      setActiveChannelId(connectedVoiceChannelId);
    }
  }, [channels, activeChannelId, connectedVoiceChannelId]);

  // Modal state
  const [createModal, setCreateModal] = useState<{ kind: 'text' | 'voice' } | null>(null);
  const [createName, setCreateName] = useState('');
  const [createPrivate, setCreatePrivate] = useState(false);
  const [createError, setCreateError] = useState('');
  const createDialogTitleId = 'create-channel-dialog-title';
  const createDialogFieldId = 'create-channel-name';

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

  return (
    <div className="flex h-[calc(100dvh-8rem)] overflow-hidden rounded-2xl border border-[var(--border)] animate-rise">
      {/* Sidebar */}
      <div className={['sm:flex shrink-0 h-full', sidebarOpen ? 'flex' : 'hidden'].join(' ')}>
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

      {/* Main content */}
      <div className="flex flex-col flex-1 h-full overflow-hidden bg-[var(--bg)]">
        {/* Mobile header with sidebar toggle */}
        <div className="sm:hidden flex items-center gap-2 border-b border-[var(--border)] px-3 py-2 shrink-0">
          <button
            className="btn-ghost px-2 py-1 text-xl leading-none"
            onClick={() => setSidebarOpen((v) => !v)}
            aria-label="Toggle sidebar"
          >
            ☰
          </button>
          {activeChannel && (
            <span className="text-sm font-medium truncate">{activeChannel.name}</span>
          )}
        </div>
        {!wsReady && (
          <div className="px-4 py-1 text-xs bg-yellow-900/30 text-yellow-300 text-center">
            Connecting…
          </div>
        )}

        <div className="flex-1 min-h-0 overflow-hidden">
          {!activeChannel ? (
            <div className="flex flex-col items-center justify-center h-full gap-2 muted">
              <span className="text-4xl">💬</span>
              <p className="text-sm">Select a channel to get started</p>
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
            />
          ) : (
            <VoiceChannelView
              key={activeChannel.id}
              channel={activeChannel}
              voicePresence={voicePresence}
              currentUserId={me.id}
              currentUsername={me.username}
              wsEvents={lastWsEvent}
            />
          )}
        </div>

      </div>

      {/* Create channel modal */}
      {createModal && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60" onClick={() => setCreateModal(null)}>
          <div
            className="panel rounded-2xl p-6 w-full max-w-sm space-y-4"
            role="dialog"
            aria-modal="true"
            aria-labelledby={createDialogTitleId}
            onClick={(event) => event.stopPropagation()}
          >
            <h2 id={createDialogTitleId} className="font-semibold text-lg">
              Create {createModal.kind === 'text' ? 'Text' : 'Voice'} Channel
            </h2>
            <div className="space-y-2">
              <label htmlFor={createDialogFieldId} className="text-sm muted">Channel name</label>
              <input
                id={createDialogFieldId}
                className="panel w-full rounded-lg px-3 py-2 text-sm"
                placeholder={createModal.kind === 'text' ? 'general' : 'Lobby'}
                value={createName}
                onChange={(e) => setCreateName(e.target.value)}
                onKeyDown={(e) => e.key === 'Enter' && handleCreateChannel()}
                autoFocus
              />
            </div>
            {me.role === 'admin' && (
              <label className="flex items-center gap-2 text-sm cursor-pointer">
                <input
                  type="checkbox"
                  checked={createPrivate}
                  onChange={(e) => setCreatePrivate(e.target.checked)}
                />
                Private channel (admins only)
              </label>
            )}
            {createError && <p className="text-sm text-red-400" role="alert">{createError}</p>}
            <div className="flex gap-2 justify-end">
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
