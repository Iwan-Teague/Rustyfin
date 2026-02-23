'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { WsYouTubeStateMessage } from '@/lib/watchPartyApi';

// Minimal YouTube IFrame API type declarations
declare global {
  interface Window {
    YT: typeof YT;
    onYouTubeIframeAPIReady: () => void;
  }
}

declare namespace YT {
  const PlayerState: {
    UNSTARTED: -1;
    ENDED: 0;
    PLAYING: 1;
    PAUSED: 2;
    BUFFERING: 3;
    CUED: 5;
  };

  class Player {
    constructor(elementId: string | HTMLElement, options: PlayerOptions);
    playVideo(): void;
    pauseVideo(): void;
    seekTo(seconds: number, allowSeekAhead?: boolean): void;
    getCurrentTime(): number;
    getPlayerState(): number;
    getVideoData(): { title: string; video_id: string };
    loadVideoById(videoId: string, startSeconds?: number): void;
    destroy(): void;
  }

  interface PlayerOptions {
    videoId?: string;
    width?: number | string;
    height?: number | string;
    playerVars?: {
      autoplay?: 0 | 1;
      controls?: 0 | 1;
      playsinline?: 0 | 1;
      rel?: 0 | 1;
      modestbranding?: 0 | 1;
    };
    events?: {
      onReady?: (event: { target: YT.Player }) => void;
      onStateChange?: (event: { target: YT.Player; data: number }) => void;
    };
  }
}

type Props = {
  roomId: string;
  ytState: WsYouTubeStateMessage | null;
  canControl: boolean;
  sendWs: (payload: Record<string, unknown>) => void;
};

function extractVideoId(input: string): string | null {
  const trimmed = input.trim();
  try {
    const url = new URL(trimmed);
    if (url.hostname.endsWith('youtube.com')) {
      const id = url.searchParams.get('v');
      if (id && /^[A-Za-z0-9_-]{11}$/.test(id)) return id;
    }
    if (url.hostname === 'youtu.be') {
      const id = url.pathname.replace('/', '').split('?')[0];
      if (/^[A-Za-z0-9_-]{11}$/.test(id)) return id;
    }
  } catch {
    // Not a URL
  }
  if (/^[A-Za-z0-9_-]{11}$/.test(trimmed)) return trimmed;
  return null;
}

export default function YouTubePlayer({ roomId, ytState, canControl, sendWs }: Props) {
  const playerRef = useRef<YT.Player | null>(null);
  const playerDivId = `yt-player-${roomId}`;
  const applyingRemoteRef = useRef(false);
  const lastVideoIdRef = useRef('');
  const canControlRef = useRef(canControl);
  const sendWsRef = useRef(sendWs);

  const [videoInput, setVideoInput] = useState('');
  const [videoTitle, setVideoTitle] = useState('');
  const [playerReady, setPlayerReady] = useState(false);

  // Keep refs in sync so event callbacks always see fresh values
  useEffect(() => { canControlRef.current = canControl; }, [canControl]);
  useEffect(() => { sendWsRef.current = sendWs; }, [sendWs]);

  const handlePlayerStateChange = useCallback((event: { target: YT.Player; data: number }) => {
    if (applyingRemoteRef.current) return;
    const player = event.target;
    const posMs = Math.floor(player.getCurrentTime() * 1000);

    if (event.data === 1 /* PLAYING */) {
      if (canControlRef.current) {
        sendWsRef.current({ type: 'play', position_ms: posMs });
      } else {
        // Viewer: revert the play
        applyingRemoteRef.current = true;
        player.pauseVideo();
        window.setTimeout(() => { applyingRemoteRef.current = false; }, 300);
      }
    } else if (event.data === 2 /* PAUSED */) {
      if (canControlRef.current) {
        sendWsRef.current({ type: 'pause', position_ms: posMs });
      }
    }
  }, []);

  const initPlayer = useCallback(() => {
    if (playerRef.current) return;
    playerRef.current = new window.YT.Player(playerDivId, {
      width: '100%',
      height: '100%',
      videoId: '',
      playerVars: {
        autoplay: 0,
        controls: 1,
        playsinline: 1,
        rel: 0,
        modestbranding: 1,
      },
      events: {
        onReady: () => setPlayerReady(true),
        onStateChange: handlePlayerStateChange,
      },
    });
  }, [playerDivId, handlePlayerStateChange]);

  // Load YouTube IFrame API
  useEffect(() => {
    if (typeof window === 'undefined') return;

    if (window.YT && window.YT.Player) {
      initPlayer();
      return;
    }

    const prev = window.onYouTubeIframeAPIReady;
    window.onYouTubeIframeAPIReady = () => {
      if (prev) prev();
      initPlayer();
    };

    if (!document.querySelector('script[src*="youtube.com/iframe_api"]')) {
      const script = document.createElement('script');
      script.src = 'https://www.youtube.com/iframe_api';
      script.async = true;
      document.head.appendChild(script);
    }
  }, [initPlayer]);

  // Destroy player on unmount
  useEffect(() => {
    return () => {
      playerRef.current?.destroy();
      playerRef.current = null;
    };
  }, []);

  // Apply remote YouTube state from WebSocket
  useEffect(() => {
    if (!ytState || !playerReady || !playerRef.current) return;
    const player = playerRef.current;

    // If video changed, load new video
    if (ytState.video_id && ytState.video_id !== lastVideoIdRef.current) {
      lastVideoIdRef.current = ytState.video_id;
      applyingRemoteRef.current = true;

      const nowMs = Date.now();
      const elapsed = nowMs - ytState.server_ts_ms;
      const projectedSecs = ytState.playing
        ? (ytState.position_ms + elapsed) / 1000
        : ytState.position_ms / 1000;

      player.loadVideoById(ytState.video_id, Math.max(0, projectedSecs));

      // After the video loads, enforce the playing/paused state
      window.setTimeout(() => {
        if (!playerRef.current) return;
        if (ytState.playing) {
          playerRef.current.playVideo();
        } else {
          playerRef.current.pauseVideo();
        }
        applyingRemoteRef.current = false;

        // Grab title after a short delay to allow the player to populate it
        window.setTimeout(() => {
          const data = playerRef.current?.getVideoData();
          if (data?.title) setVideoTitle(data.title);
        }, 2000);
      }, 1500);
      return;
    }

    // Apply play/pause/seek sync for same video
    applyingRemoteRef.current = true;
    const nowMs = Date.now();
    const elapsed = nowMs - ytState.server_ts_ms;
    const projectedMs = ytState.playing ? ytState.position_ms + elapsed : ytState.position_ms;
    const projectedSecs = projectedMs / 1000;
    const currentSecs = player.getCurrentTime();

    if (Math.abs(currentSecs - projectedSecs) > 1.5) {
      player.seekTo(projectedSecs, true);
    }

    const playerState = player.getPlayerState();
    if (ytState.playing && playerState !== 1 /* PLAYING */) {
      player.playVideo();
    } else if (!ytState.playing && playerState === 1 /* PLAYING */) {
      player.pauseVideo();
    }

    window.setTimeout(() => { applyingRemoteRef.current = false; }, 300);
  }, [ytState, playerReady]);

  // Drift correction every 3 seconds
  useEffect(() => {
    if (!playerReady || !ytState) return;

    const interval = window.setInterval(() => {
      if (!ytState || !playerRef.current || applyingRemoteRef.current) return;
      const player = playerRef.current;
      if (player.getPlayerState() !== 1 /* PLAYING */) return;

      const nowMs = Date.now();
      const elapsed = nowMs - ytState.server_ts_ms;
      const projectedSecs = ytState.playing
        ? (ytState.position_ms + elapsed) / 1000
        : ytState.position_ms / 1000;

      if (Math.abs(player.getCurrentTime() - projectedSecs) > 1.5) {
        applyingRemoteRef.current = true;
        player.seekTo(projectedSecs, true);
        window.setTimeout(() => { applyingRemoteRef.current = false; }, 300);
      }
    }, 3000);

    return () => window.clearInterval(interval);
  }, [ytState, playerReady]);

  function handleVideoSubmit() {
    const id = extractVideoId(videoInput);
    if (!id) return;
    setVideoInput('');
    sendWs({ type: 'change_video', video_id: id });
  }

  const isValidInput = extractVideoId(videoInput) !== null;
  const hasVideo = !!(ytState?.video_id);

  return (
    <section className="space-y-4">
      {canControl && (
        <div className="flex gap-2">
          <input
            type="text"
            value={videoInput}
            onChange={(e) => setVideoInput(e.target.value)}
            onKeyDown={(e) => { if (e.key === 'Enter') handleVideoSubmit(); }}
            placeholder="Paste YouTube URL or video ID…"
            className="input flex-1 px-3 py-2 text-sm"
          />
          <button
            type="button"
            className="btn-primary px-4 py-2 text-sm disabled:opacity-50"
            onClick={handleVideoSubmit}
            disabled={!isValidInput}
          >
            Load
          </button>
        </div>
      )}

      <div
        className="tile overflow-hidden rounded-2xl border border-white/10 bg-black relative"
        style={{ aspectRatio: '16/9', width: '100%' }}
      >
        {!hasVideo && (
          <div className="absolute inset-0 flex items-center justify-center z-10 pointer-events-none">
            <p className="text-sm muted text-center px-6">
              {canControl
                ? 'Paste a YouTube URL or video ID above to get started.'
                : 'Waiting for host to select a video…'}
            </p>
          </div>
        )}
        <div id={playerDivId} className="h-full w-full" />
      </div>

      {videoTitle && (
        <p className="text-sm font-medium truncate muted">▶ {videoTitle}</p>
      )}

      {!canControl && (
        <p className="text-xs muted">
          You are a viewer — only the host and controllers can change the video.
        </p>
      )}
    </section>
  );
}
