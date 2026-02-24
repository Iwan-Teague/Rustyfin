'use client';

import { useState } from 'react';

interface Props {
  channelName: string;
  muted: boolean;
  deafened: boolean;
  hasLocalStream: boolean;
  onToggleMute: () => void;
  onToggleDeafen: () => void;
  onLeave: () => void;
}

export default function VoiceBar({
  channelName,
  muted,
  deafened,
  hasLocalStream,
  onToggleMute,
  onToggleDeafen,
  onLeave,
}: Props) {
  const [confirmingDisconnect, setConfirmingDisconnect] = useState(false);

  const handleDisconnect = () => {
    setConfirmingDisconnect(false);
    onLeave();
  };

  return (
    <>
      <div className="fixed bottom-5 left-1/2 -translate-x-1/2 z-50 flex items-center gap-3 px-4 py-2.5 rounded-2xl bg-[var(--surface)] border border-[var(--border)] shadow-xl">
        <span className="w-2 h-2 rounded-full bg-green-400 animate-pulse shrink-0" />
        <span className="text-sm font-medium">{channelName}</span>
        <div className="w-px h-4 bg-[var(--border)]" />
        {hasLocalStream ? (
          <button
            onClick={onToggleMute}
            className="btn-ghost px-2 py-1 text-sm leading-none"
            title={muted ? 'Unmute' : 'Mute'}
          >
            {muted ? 'Unmute' : 'Mute'}
          </button>
        ) : (
          <span className="text-sm muted px-1" title="No microphone — listening only">
            Listening
          </span>
        )}
        <button
          onClick={onToggleDeafen}
          className={`btn-ghost px-2 py-1 text-sm leading-none ${deafened ? 'text-[var(--orange-soft)]' : ''}`}
          title={deafened ? 'Undeafen (hear others)' : 'Deafen (mute others locally)'}
        >
          {deafened ? 'Undeafen' : 'Deafen'}
        </button>
        <button
          onClick={() => setConfirmingDisconnect(true)}
          className="btn-ghost px-2 py-1 text-base leading-none text-red-400 hover:text-red-300"
          title="Disconnect from voice"
        >
          ✕
        </button>
      </div>

      {confirmingDisconnect && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
          <div className="panel rounded-2xl p-6 w-full max-w-sm space-y-4">
            <h2 className="font-semibold text-lg">Disconnect from &ldquo;{channelName}&rdquo;?</h2>
            <p className="text-sm muted">
              You will leave the voice channel and your connection will be closed.
            </p>
            <div className="flex gap-2 justify-end">
              <button
                onClick={() => setConfirmingDisconnect(false)}
                className="btn-ghost px-4 py-2 text-sm"
              >
                Cancel
              </button>
              <button
                onClick={handleDisconnect}
                className="btn-primary px-4 py-2 text-sm bg-red-500 hover:bg-red-600"
              >
                Disconnect
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  );
}
