'use client';

interface Props {
  channelName: string;
  muted: boolean;
  hasLocalStream: boolean;
  onToggleMute: () => void;
  onLeave: () => void;
}

export default function VoiceBar({ channelName, muted, hasLocalStream, onToggleMute, onLeave }: Props) {
  return (
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
        onClick={onLeave}
        className="btn-ghost px-2 py-1 text-base leading-none text-red-400 hover:text-red-300"
        title="Disconnect from voice"
      >
        ✕
      </button>
    </div>
  );
}
