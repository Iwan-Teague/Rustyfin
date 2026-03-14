'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import Image from 'next/image';
import {
  WsYouTubeStateMessage,
  YouTubeSearchResult,
  lookupYouTubeVideos,
  searchYouTubeVideos,
} from '@/lib/watchPartyApi';
import { findDataDeleteTarget, playTelegramDeleteAnimation } from '@/lib/deleteAnimation';
import { clientErrorMessage } from '@/lib/errors';
import ClearSearchButton from './ClearSearchButton';

type YTPlayerStateMap = {
  UNSTARTED: -1;
  ENDED: 0;
  PLAYING: 1;
  PAUSED: 2;
  BUFFERING: 3;
  CUED: 5;
};

type YTPlayer = {
  playVideo(): void;
  pauseVideo(): void;
  seekTo(seconds: number, allowSeekAhead?: boolean): void;
  getCurrentTime(): number;
  getPlayerState(): number;
  getVideoData(): { title: string; video_id: string };
  loadVideoById(videoId: string, startSeconds?: number): void;
  cueVideoById(videoId: string, startSeconds?: number): void;
  destroy(): void;
};

type YTPlayerOptions = {
  videoId?: string;
  width?: number | string;
  height?: number | string;
  playerVars?: {
    autoplay?: 0 | 1;
    controls?: 0 | 1;
    playsinline?: 0 | 1;
    rel?: 0 | 1;
    modestbranding?: 0 | 1;
    enablejsapi?: 0 | 1;
    iv_load_policy?: 1 | 3;
    origin?: string;
  };
  host?: string;
  events?: {
    onReady?: (event: { target: YTPlayer }) => void;
    onStateChange?: (event: { target: YTPlayer; data: number }) => void;
    onError?: (event: { target: YTPlayer; data: number }) => void;
  };
};

type YouTubeIframeApi = {
  PlayerState: YTPlayerStateMap;
  Player: new (elementId: string | HTMLElement, options: YTPlayerOptions) => YTPlayer;
};

// Minimal YouTube IFrame API type declarations
declare global {
  interface Window {
    YT: YouTubeIframeApi;
    onYouTubeIframeAPIReady: () => void;
  }
}

type Props = {
  roomId: string;
  ytState: WsYouTubeStateMessage | null;
  canControl: boolean;
  canQueue: boolean;
  wsConnected: boolean;
  sendWs: (payload: Record<string, unknown>) => boolean;
};

const YOUTUBE_ID_RE = /^[A-Za-z0-9_-]{11}$/;

function isValidVideoId(value: string): boolean {
  return YOUTUBE_ID_RE.test(value);
}

function mapPlayerErrorCode(code: number): string {
  if (code === 2) return 'YouTube rejected this video identifier. Use a valid YouTube URL or ID.';
  if (code === 5) return 'The browser could not play this embedded YouTube stream.';
  if (code === 100) return 'This YouTube video is unavailable or private.';
  if (code === 101 || code === 150) {
    return 'This YouTube video cannot be embedded by the uploader. Try another video.';
  }
  if (code === 153) {
    return 'YouTube blocked this request due to missing referrer/client metadata. Open the room over HTTPS and try again.';
  }
  return 'YouTube player failed to load this video.';
}

function clampSeconds(value: number): number {
  if (!Number.isFinite(value)) return 0;
  if (value < 0) return 0;
  return value;
}

function mapStateCode(code: number): string {
  if (code === -1) return 'UNSTARTED';
  if (code === 0) return 'ENDED';
  if (code === 1) return 'PLAYING';
  if (code === 2) return 'PAUSED';
  if (code === 3) return 'BUFFERING';
  if (code === 5) return 'CUED';
  return `UNKNOWN(${code})`;
}

function formatViewCount(value: number): string {
  return new Intl.NumberFormat(undefined, {
    notation: 'compact',
    maximumFractionDigits: 1,
  }).format(value);
}

export default function YouTubePlayer({
  roomId,
  ytState,
  canControl,
  canQueue,
  wsConnected,
  sendWs,
}: Props) {
  const playerRef = useRef<YTPlayer | null>(null);
  const playerDivId = `yt-player-${roomId}`;
  const applyingRemoteRef = useRef(false);
  const lastVideoIdRef = useRef('');
  const pendingVideoIdRef = useRef<string | null>(null);
  const pendingVideoAckTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pendingSearchQueryRef = useRef<string | null>(null);
  const pendingSearchAckTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const searchPanelRef = useRef<HTMLDivElement | null>(null);
  const searchResultsListRef = useRef<HTMLUListElement | null>(null);
  const lastSyncedSharedSearchQueryRef = useRef<string | null>(null);
  const pendingQueueTimeoutsRef = useRef<Record<string, ReturnType<typeof setTimeout>>>({});
  const queueLookupFailedRef = useRef<Set<string>>(new Set());
  const currentVideoLookupInFlightRef = useRef<string | null>(null);
  const canControlRef = useRef(canControl);
  const sendWsRef = useRef(sendWs);
  // Always mirrors the latest ytState so event callbacks can read it without stale closure values
  const ytStateRef = useRef<WsYouTubeStateMessage | null>(ytState);

  const [videoTitle, setVideoTitle] = useState('');
  const [playerReady, setPlayerReady] = useState(false);
  const [playerError, setPlayerError] = useState('');
  const [searchInput, setSearchInput] = useState('');
  const [queueMetaById, setQueueMetaById] = useState<Record<string, YouTubeSearchResult>>({});
  const [pendingQueueById, setPendingQueueById] = useState<Record<string, boolean>>({});
  const [searching, setSearching] = useState(false);
  const [searchResultsCollapsed, setSearchResultsCollapsed] = useState(false);
  const [searchError, setSearchError] = useState('');
  const [fallbackSearchResults, setFallbackSearchResults] = useState<YouTubeSearchResult[] | null>(null);
  const sharedSearchQuery = ytState?.search_query ?? '';
  const sharedSearchResults = ytState?.search_results ?? [];
  const searchResults = fallbackSearchResults ?? sharedSearchResults;

  const logDebug = useCallback((message: string) => {
    if (typeof window !== 'undefined') {
      console.info(`[watch-party:youtube:${roomId}] ${message}`);
    }
  }, [roomId]);

  const clearPendingVideoAck = useCallback(() => {
    if (pendingVideoAckTimeoutRef.current !== null) {
      clearTimeout(pendingVideoAckTimeoutRef.current);
      pendingVideoAckTimeoutRef.current = null;
    }
    pendingVideoIdRef.current = null;
  }, []);

  const clearPendingSearchAck = useCallback(() => {
    if (pendingSearchAckTimeoutRef.current !== null) {
      clearTimeout(pendingSearchAckTimeoutRef.current);
      pendingSearchAckTimeoutRef.current = null;
    }
    pendingSearchQueryRef.current = null;
  }, []);

  const clearPendingQueueMarker = useCallback((videoId: string) => {
    const timer = pendingQueueTimeoutsRef.current[videoId];
    if (timer) {
      clearTimeout(timer);
      delete pendingQueueTimeoutsRef.current[videoId];
    }
    setPendingQueueById((prev) => {
      if (!prev[videoId]) return prev;
      const next = { ...prev };
      delete next[videoId];
      return next;
    });
  }, []);

  const markPendingQueue = useCallback((videoId: string) => {
    clearPendingQueueMarker(videoId);
    setPendingQueueById((prev) => ({ ...prev, [videoId]: true }));
    pendingQueueTimeoutsRef.current[videoId] = setTimeout(() => {
      clearPendingQueueMarker(videoId);
    }, 6000);
  }, [clearPendingQueueMarker]);

  // Keep refs in sync so event callbacks always see fresh values
  useEffect(() => { canControlRef.current = canControl; }, [canControl]);
  useEffect(() => { sendWsRef.current = sendWs; }, [sendWs]);
  useEffect(() => { ytStateRef.current = ytState; }, [ytState]);

  useEffect(() => {
    if (typeof window === 'undefined') return;
    logDebug(
      `component mounted origin=${window.location.origin} secure_context=${window.isSecureContext}`,
    );
  }, [logDebug]);

  useEffect(() => {
    if ((!canControl && !canQueue) || wsConnected) return;
    setPlayerError('Realtime connection is offline. Reconnect to load or control YouTube playback.');
    logDebug('ws disconnected while youtube controls are available');
  }, [canControl, canQueue, wsConnected, logDebug]);

  useEffect(() => {
    if (!ytState?.video_id) return;
    if (pendingVideoIdRef.current && pendingVideoIdRef.current === ytState.video_id) {
      logDebug(`received youtube_state for requested video_id=${ytState.video_id}`);
      clearPendingVideoAck();
    }
  }, [ytState?.video_id, clearPendingVideoAck, logDebug]);

  useEffect(() => {
    const currentVideoId = ytState?.video_id || '';
    const queueSet = new Set(ytState?.queue ?? []);
    for (const videoId of Object.keys(pendingQueueById)) {
      if (videoId === currentVideoId || queueSet.has(videoId)) {
        clearPendingQueueMarker(videoId);
      }
    }
  }, [ytState?.queue, ytState?.video_id, pendingQueueById, clearPendingQueueMarker]);

  useEffect(() => {
    if (lastSyncedSharedSearchQueryRef.current === null) {
      lastSyncedSharedSearchQueryRef.current = sharedSearchQuery;
      if (sharedSearchQuery) {
        setSearchInput(sharedSearchQuery);
      }
      return;
    }

    if (sharedSearchQuery !== lastSyncedSharedSearchQueryRef.current) {
      lastSyncedSharedSearchQueryRef.current = sharedSearchQuery;
      setSearchInput(sharedSearchQuery);
    }
  }, [sharedSearchQuery]);

  useEffect(() => {
    if (!ytState) return;

    if (sharedSearchResults.length > 0) {
      setQueueMetaById((prev) => {
        const next = { ...prev };
        for (const result of sharedSearchResults) {
          next[result.video_id] = result;
        }
        return next;
      });
    }

    if (pendingSearchQueryRef.current && sharedSearchQuery === pendingSearchQueryRef.current) {
      clearPendingSearchAck();
      setSearching(false);
      setFallbackSearchResults(null);
      setSearchError(sharedSearchResults.length === 0 ? 'No YouTube results found for that query.' : '');
      window.requestAnimationFrame(() => {
        searchResultsListRef.current?.scrollTo({ top: 0, behavior: 'auto' });
      });
      logDebug(`youtube shared search update query="${sharedSearchQuery}" results=${sharedSearchResults.length}`);
    }
  }, [ytState, sharedSearchQuery, sharedSearchResults, clearPendingSearchAck, logDebug]);

  const handlePlayerStateChange = useCallback((event: { target: YTPlayer; data: number }) => {
    if (applyingRemoteRef.current) return;
    const player = event.target;
    const posMs = Math.floor(player.getCurrentTime() * 1000);
    const data = player.getVideoData();
    if (data?.title) setVideoTitle(data.title);
    logDebug(
      `player onStateChange state=${mapStateCode(event.data)} position_ms=${posMs} video_id=${data?.video_id || 'none'}`,
    );

    if (event.data === 1 /* PLAYING */) {
      if (canControlRef.current) {
        const sent = sendWsRef.current({ type: 'play', position_ms: posMs });
        if (!sent) {
          setPlayerError('Failed to send play command. Reconnect the room and retry.');
          logDebug('failed to send play command over websocket');
        }
      } else {
        // Viewer: only revert play when the remote state is paused/unset.
        // If the remote state says "playing", the PLAYING event is expected (buffering
        // just finished after a remote sync) and must NOT be reverted — doing so
        // would permanently pause the video for slow-buffering clients.
        if (!ytStateRef.current?.playing) {
          applyingRemoteRef.current = true;
          player.pauseVideo();
          window.setTimeout(() => { applyingRemoteRef.current = false; }, 300);
        }
      }
    } else if (event.data === 2 /* PAUSED */) {
      if (canControlRef.current) {
        const sent = sendWsRef.current({ type: 'pause', position_ms: posMs });
        if (!sent) {
          setPlayerError('Failed to send pause command. Reconnect the room and retry.');
          logDebug('failed to send pause command over websocket');
        }
      }
    } else if (event.data === 0 /* ENDED */) {
      if (canControlRef.current) {
        const endedVideoId = data?.video_id || ytStateRef.current?.video_id || '';
        if (endedVideoId) {
          const sent = sendWsRef.current({
            type: 'advance_queue',
            expected_video_id: endedVideoId,
          });
          if (!sent) {
            setPlayerError('Failed to auto-advance YouTube queue. Reconnect and retry.');
            logDebug(`failed to send advance_queue command video_id=${endedVideoId}`);
          } else {
            logDebug(`sent advance_queue command expected_video_id=${endedVideoId}`);
          }
        }
      }
    }
  }, [logDebug]);

  const initPlayer = useCallback(() => {
    if (playerRef.current) return;
    const origin = window.location.origin;
    logDebug(`initializing iframe player origin=${origin}`);
    playerRef.current = new window.YT.Player(playerDivId, {
      width: '100%',
      height: '100%',
      // No videoId here — passing '' causes YouTube to serve an error page (/embed/ with no
      // video ID) which means the IFrame API JS never initialises, onReady never fires, and
      // playerReady stays false permanently.  Videos are loaded later via cueVideoById /
      // loadVideoById once ytState with a video_id arrives from the WebSocket.
      host: 'https://www.youtube.com',
      playerVars: {
        autoplay: 0,
        controls: 1,
        playsinline: 1,
        rel: 0,
        modestbranding: 1,
        enablejsapi: 1,
        iv_load_policy: 3,
        origin,
      },
      events: {
        onReady: () => {
          setPlayerReady(true);
          setPlayerError('');
          logDebug('iframe player ready');
        },
        onStateChange: handlePlayerStateChange,
        onError: (event) => {
          const playerData = event.target.getVideoData();
          const reason = mapPlayerErrorCode(event.data);
          setPlayerError(`${reason} (code ${event.data})`);
          logDebug(
            `iframe player error code=${event.data} reason="${reason}" video_id=${playerData?.video_id || 'none'} title="${playerData?.title || ''}"`,
          );
        },
      },
    });
  }, [playerDivId, handlePlayerStateChange, logDebug]);

  // Load YouTube IFrame API
  useEffect(() => {
    if (typeof window === 'undefined') return;

    if (window.YT && window.YT.Player) {
      logDebug('iframe API already available on window');
      initPlayer();
      return;
    }

    const prev = window.onYouTubeIframeAPIReady;
    window.onYouTubeIframeAPIReady = () => {
      logDebug('window.onYouTubeIframeAPIReady fired');
      if (prev) prev();
      initPlayer();
    };

    if (!document.querySelector('script[src*="youtube.com/iframe_api"]')) {
      logDebug('injecting iframe_api script');
      const script = document.createElement('script');
      script.src = 'https://www.youtube.com/iframe_api';
      script.async = true;
      script.onload = () => {
        logDebug('iframe_api script loaded');
      };
      script.onerror = () => {
        const message =
          'Failed to load YouTube IFrame API script. Check network, DNS, or content blockers.';
        setPlayerError(message);
        logDebug('iframe_api script load failed');
      };
      document.head.appendChild(script);
    } else {
      logDebug('iframe_api script already present in document');
    }
  }, [initPlayer, logDebug]);

  // Destroy player on unmount
  useEffect(() => {
    return () => {
      clearPendingVideoAck();
      clearPendingSearchAck();
      for (const timer of Object.values(pendingQueueTimeoutsRef.current)) {
        clearTimeout(timer);
      }
      pendingQueueTimeoutsRef.current = {};
      playerRef.current?.destroy();
      playerRef.current = null;
      logDebug('component unmounted and player destroyed');
    };
  }, [clearPendingSearchAck, clearPendingVideoAck, logDebug]);

  // Apply remote YouTube state from WebSocket
  useEffect(() => {
    if (!ytState || !playerReady || !playerRef.current) return;
    const player = playerRef.current;

    // If video changed, load new video
    if (ytState.video_id && ytState.video_id !== lastVideoIdRef.current) {
      lastVideoIdRef.current = ytState.video_id;
      applyingRemoteRef.current = true;
      setPlayerError('');
      setVideoTitle('');
      logDebug(
        `applying remote video change video_id=${ytState.video_id} playing=${ytState.playing} position_ms=${ytState.position_ms}`,
      );

      const nowMs = Date.now();
      const elapsed = nowMs - ytState.server_ts_ms;
      const projectedSecs = ytState.playing
        ? clampSeconds((ytState.position_ms + elapsed) / 1000)
        : clampSeconds(ytState.position_ms / 1000);

      if (ytState.playing) {
        player.loadVideoById(ytState.video_id, projectedSecs);
      } else {
        player.cueVideoById(ytState.video_id, projectedSecs);
      }

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
    const projectedSecs = clampSeconds(projectedMs / 1000);
    const currentSecs = player.getCurrentTime();

    if (Math.abs(currentSecs - projectedSecs) > 1.5) {
      logDebug(
        `sync correction seek current_s=${currentSecs.toFixed(2)} target_s=${projectedSecs.toFixed(2)}`,
      );
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
        ? clampSeconds((ytState.position_ms + elapsed) / 1000)
        : clampSeconds(ytState.position_ms / 1000);

      if (Math.abs(player.getCurrentTime() - projectedSecs) > 1.5) {
        logDebug(
          `drift correction seek current_s=${player.getCurrentTime().toFixed(2)} target_s=${projectedSecs.toFixed(2)}`,
        );
        applyingRemoteRef.current = true;
        player.seekTo(projectedSecs, true);
        window.setTimeout(() => { applyingRemoteRef.current = false; }, 300);
      }
    }, 3000);

    return () => window.clearInterval(interval);
  }, [ytState, playerReady]);

  const queue = ytState?.queue ?? [];
  const queueSet = new Set(queue);

  const isQueueBlocked = useCallback((videoId: string) => {
    if (!isValidVideoId(videoId)) return false;
    if (pendingQueueById[videoId]) return true;
    if (ytState?.video_id === videoId) return true;
    return queueSet.has(videoId);
  }, [pendingQueueById, queueSet, ytState?.video_id]);

  const submitVideoId = useCallback((videoId: string, mode: 'load' | 'queue', source: string) => {
    if (!isValidVideoId(videoId)) {
      setPlayerError('Invalid YouTube video ID.');
      logDebug(`${mode} rejected from ${source}: invalid video id`);
      return false;
    }
    if (!wsConnected) {
      setPlayerError('Cannot load video while realtime connection is offline.');
      logDebug(`${mode} rejected from ${source}: websocket is disconnected`);
      return false;
    }

    if (mode === 'load' && !canControl) {
      setPlayerError('Only room admins can load the current YouTube video.');
      logDebug(`load rejected from ${source}: user lacks control permission`);
      return false;
    }
    if (mode === 'queue' && !canQueue) {
      setPlayerError('You must join the room to add YouTube videos to queue.');
      logDebug(`queue rejected from ${source}: user is not allowed to queue`);
      return false;
    }
    if (mode === 'queue' && isQueueBlocked(videoId)) {
      setPlayerError('This video is already queued (or currently playing).');
      logDebug(`queue rejected from ${source}: already queued video_id=${videoId}`);
      return false;
    }
    setPlayerError('');

    const payload =
      mode === 'load'
        ? { type: 'change_video', video_id: videoId }
        : { type: 'queue_video', video_id: videoId };
    if (mode === 'queue') {
      markPendingQueue(videoId);
    }
    logDebug(`sending ${payload.type} command video_id=${videoId} source=${source}`);
    const sent = sendWs(payload);
    if (!sent) {
      if (mode === 'queue') {
        clearPendingQueueMarker(videoId);
      }
      setPlayerError(
        mode === 'load'
          ? 'Failed to send video change command. Reconnect the room and retry.'
          : 'Failed to queue YouTube video. Reconnect the room and retry.',
      );
      logDebug(`${payload.type} send failed source=${source}`);
      return false;
    }

    if (mode === 'queue') {
      logDebug(`queued video_id=${videoId} source=${source}`);
      return true;
    }

    setVideoTitle('');
    clearPendingVideoAck();
    pendingVideoIdRef.current = videoId;
    pendingVideoAckTimeoutRef.current = setTimeout(() => {
      if (pendingVideoIdRef.current !== videoId) return;
      const message =
        'No room state update received after loading this video. Check websocket/auth logs below.';
      setPlayerError(message);
      logDebug(
        `timeout waiting for youtube_state acknowledgement video_id=${videoId} source=${source}`,
      );
    }, 8000);
    return true;
  }, [
    canControl,
    canQueue,
    clearPendingQueueMarker,
    clearPendingVideoAck,
    isQueueBlocked,
    logDebug,
    markPendingQueue,
    sendWs,
    wsConnected,
  ]);

  const resetSearchViewport = useCallback(() => {
    searchPanelRef.current?.scrollIntoView({ block: 'start', behavior: 'smooth' });
    searchResultsListRef.current?.scrollTo({ top: 0, behavior: 'auto' });
  }, []);

  async function runSearch() {
    const query = searchInput.trim();
    if (query.length < 2) {
      setSearchError('Enter at least 2 characters to search.');
      return;
    }
    if (!wsConnected) {
      setSearchError('Cannot search while realtime connection is offline.');
      return;
    }
    if (!canControl && !canQueue) {
      setSearchError('You must join the room to search videos.');
      return;
    }

    const sent = sendWs({ type: 'search_youtube', query });
    if (!sent) {
      setSearchError('Failed to send search command. Reconnect and retry.');
      logDebug(`youtube search send failed query="${query}"`);
      return;
    }

    clearPendingSearchAck();
    setFallbackSearchResults(null);
    pendingSearchQueryRef.current = query;
    pendingSearchAckTimeoutRef.current = setTimeout(() => {
      if (pendingSearchQueryRef.current !== query) return;
      logDebug(`youtube search timeout waiting for shared update query="${query}" -> falling back to direct API search`);
      void (async () => {
        try {
          const directResults = await searchYouTubeVideos(roomId, query, 12);
          if (pendingSearchQueryRef.current !== query) return;
          clearPendingSearchAck();
          setSearching(false);
          setFallbackSearchResults(directResults);
          setSearchError(directResults.length === 0 ? 'No YouTube results found for that query.' : '');
          if (directResults.length > 0) {
            setQueueMetaById((prev) => {
              const next = { ...prev };
              for (const result of directResults) {
                next[result.video_id] = result;
              }
              return next;
            });
          }
          window.requestAnimationFrame(() => {
            searchResultsListRef.current?.scrollTo({ top: 0, behavior: 'auto' });
          });
          logDebug(
            `youtube direct-search fallback query="${query}" results=${directResults.length}`,
          );
        } catch (err: unknown) {
          if (pendingSearchQueryRef.current !== query) return;
          clearPendingSearchAck();
          setSearching(false);
          setSearchError(
            clientErrorMessage(err, 'No shared search update received yet. Check websocket/debug trace and retry.'),
          );
          logDebug(
            `youtube search fallback failed query="${query}" error=${clientErrorMessage(err, 'fallback failed')}`,
          );
        }
      })();
    }, 8000);
    setSearchError('');
    setSearching(true);
    setSearchResultsCollapsed(false);
    resetSearchViewport();
    logDebug(`youtube search requested query="${query}"`);
  }

  const clearSearchInput = useCallback(() => {
    clearPendingSearchAck();
    setFallbackSearchResults(null);
    setSearchInput('');
    setSearchError('');
    setSearching(false);
    setSearchResultsCollapsed(false);
    if (wsConnected && (canControl || canQueue)) {
      const sent = sendWs({ type: 'search_youtube', query: '' });
      if (!sent) {
        logDebug('youtube clear search command send failed');
      } else {
        logDebug('youtube shared search cleared');
      }
    }
  }, [canControl, canQueue, clearPendingSearchAck, logDebug, sendWs, wsConnected]);

  const requestQueuePlayNow = useCallback((queueIndex: number) => {
    if (!canControl) {
      setPlayerError('Only room admins can play queued videos now.');
      return;
    }
    if (!wsConnected) {
      setPlayerError('Cannot control queue while realtime connection is offline.');
      return;
    }
    const sent = sendWs({ type: 'play_queued_video', queue_index: queueIndex });
    if (!sent) {
      setPlayerError('Failed to play queued video now. Reconnect and retry.');
      logDebug(`play_queued_video send failed queue_index=${queueIndex}`);
      return;
    }
    setPlayerError('');
    logDebug(`play_queued_video sent queue_index=${queueIndex}`);
  }, [canControl, wsConnected, sendWs, logDebug]);

  const requestQueueRemove = useCallback(async (queueIndex: number) => {
    if (!canQueue) {
      setPlayerError('You must join the room to edit queue.');
      return;
    }
    if (!wsConnected) {
      setPlayerError('Cannot edit queue while realtime connection is offline.');
      return;
    }
    const target = findDataDeleteTarget('data-youtube-queue-index', String(queueIndex));
    await playTelegramDeleteAnimation(target);
    const sent = sendWs({ type: 'remove_queued_video', queue_index: queueIndex });
    if (!sent) {
      setPlayerError('Failed to remove queued video. Reconnect and retry.');
      logDebug(`remove_queued_video send failed queue_index=${queueIndex}`);
      return;
    }
    setPlayerError('');
    logDebug(`remove_queued_video sent queue_index=${queueIndex}`);
  }, [canQueue, wsConnected, sendWs, logDebug]);

  const requestQueueMove = useCallback((fromIndex: number, toIndex: number) => {
    if (!canQueue) {
      setPlayerError('You must join the room to edit queue.');
      return;
    }
    if (!wsConnected) {
      setPlayerError('Cannot edit queue while realtime connection is offline.');
      return;
    }
    if (toIndex < 0 || toIndex >= queue.length || fromIndex === toIndex) {
      return;
    }
    const sent = sendWs({
      type: 'move_queued_video',
      from_index: fromIndex,
      to_index: toIndex,
    });
    if (!sent) {
      setPlayerError('Failed to reorder queue. Reconnect and retry.');
      logDebug(`move_queued_video send failed from=${fromIndex} to=${toIndex}`);
      return;
    }
    setPlayerError('');
    logDebug(`move_queued_video sent from=${fromIndex} to=${toIndex}`);
  }, [canQueue, wsConnected, queue.length, sendWs, logDebug]);

  useEffect(() => {
    if (queue.length === 0) return;
    const uniqueMissing = Array.from(new Set(queue)).filter(
      (videoId) =>
        isValidVideoId(videoId) &&
        !queueMetaById[videoId] &&
        !queueLookupFailedRef.current.has(videoId),
    );

    if (uniqueMissing.length === 0) return;

    let cancelled = false;
    const requestIds = uniqueMissing.slice(0, 12);

    (async () => {
      try {
        const resolved = await lookupYouTubeVideos(roomId, requestIds);
        if (cancelled) return;

        if (resolved.length > 0) {
          setQueueMetaById((prev) => {
            const next = { ...prev };
            for (const result of resolved) {
              next[result.video_id] = result;
            }
            return next;
          });
        }

        const foundIds = new Set(resolved.map((result) => result.video_id));
        for (const videoId of requestIds) {
          if (!foundIds.has(videoId)) {
            queueLookupFailedRef.current.add(videoId);
          }
        }
        logDebug(
          `youtube queue metadata resolved requested=${requestIds.length} resolved=${resolved.length}`,
        );
      } catch (err: unknown) {
        if (cancelled) return;
        for (const videoId of requestIds) {
          queueLookupFailedRef.current.add(videoId);
        }
        logDebug(
          `youtube queue metadata lookup failed error=${clientErrorMessage(err, 'lookup failed')}`,
        );
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [queue, queueMetaById, roomId, logDebug]);

  useEffect(() => {
    const currentVideoId = ytState?.video_id;
    if (!currentVideoId || !isValidVideoId(currentVideoId)) return;
    if (queueMetaById[currentVideoId]) return;
    if (queueLookupFailedRef.current.has(currentVideoId)) return;
    if (currentVideoLookupInFlightRef.current === currentVideoId) return;

    let cancelled = false;
    currentVideoLookupInFlightRef.current = currentVideoId;

    (async () => {
      try {
        const resolved = await lookupYouTubeVideos(roomId, [currentVideoId]);
        if (cancelled) return;

        if (resolved.length > 0) {
          setQueueMetaById((prev) => ({ ...prev, [resolved[0].video_id]: resolved[0] }));
          logDebug(`youtube current video metadata resolved video_id=${currentVideoId}`);
        } else {
          queueLookupFailedRef.current.add(currentVideoId);
          logDebug(`youtube current video metadata unavailable video_id=${currentVideoId}`);
        }
      } catch (err: unknown) {
        if (cancelled) return;
        queueLookupFailedRef.current.add(currentVideoId);
        logDebug(
          `youtube current video metadata lookup failed video_id=${currentVideoId} error=${clientErrorMessage(err, 'lookup failed')}`,
        );
      } finally {
        if (currentVideoLookupInFlightRef.current === currentVideoId) {
          currentVideoLookupInFlightRef.current = null;
        }
      }
    })();

    return () => {
      cancelled = true;
      if (currentVideoLookupInFlightRef.current === currentVideoId) {
        currentVideoLookupInFlightRef.current = null;
      }
    };
  }, [ytState?.video_id, queueMetaById, roomId, logDebug]);

  const currentVideoId = ytState?.video_id ?? '';
  const currentVideoMeta = currentVideoId ? queueMetaById[currentVideoId] : undefined;
  const currentVideoTitle = videoTitle || currentVideoMeta?.title || '';

  return (
    <section className="space-y-4">
      <div
        className="tile overflow-hidden rounded-2xl border border-white/10 bg-black relative"
        style={{ aspectRatio: '16/9', width: '100%' }}
      >
        {!playerReady && (
          <div className="absolute inset-0 flex items-center justify-center z-20 pointer-events-none">
            <p className="text-sm muted text-center px-6">Initializing YouTube player…</p>
          </div>
        )}
        <div id={playerDivId} className="h-full w-full" />
      </div>

      {(currentVideoTitle || currentVideoId) && (
        <div className="space-y-0.5">
          {currentVideoTitle && (
            <p className="text-sm font-medium truncate muted">{currentVideoTitle}</p>
          )}
          {currentVideoMeta ? (
            <p className="text-xs muted truncate">
              {currentVideoMeta.channel}
              {typeof currentVideoMeta.view_count === 'number'
                ? ` • ${formatViewCount(currentVideoMeta.view_count)} views`
                : ''}
            </p>
          ) : (
            <p className="text-xs muted truncate">Loading channel and view information…</p>
          )}
        </div>
      )}

      {(canControl || canQueue) && (
        <div ref={searchPanelRef} className="panel-soft rounded-xl px-3 py-3 space-y-3">
          <div className="flex flex-col gap-2 sm:flex-row">
            <div className="relative flex-1">
              <input
                type="text"
                value={searchInput}
                onChange={(e) => setSearchInput(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') {
                    void runSearch();
                  }
                }}
                placeholder="Search YouTube or paste a video URL…"
                className="input w-full px-3 py-2 pr-10 text-sm"
              />
              {searchInput.trim().length > 0 && (
                <ClearSearchButton
                  onClick={clearSearchInput}
                  className="absolute right-2 top-1/2 -translate-y-1/2"
                />
              )}
            </div>
            <button
              type="button"
              className="btn-secondary px-4 py-2 text-sm disabled:opacity-50"
              onClick={() => {
                void runSearch();
              }}
              disabled={searching || searchInput.trim().length < 2}
            >
              {searching ? 'Searching…' : 'Search'}
            </button>
          </div>

          {searchError && (
            <p className="text-xs text-red-300">{searchError}</p>
          )}

          {searchResults.length > 0 && (
            <div className="space-y-2">
              <button
                type="button"
                className="btn-secondary px-3 py-1.5 text-xs"
                onClick={() => setSearchResultsCollapsed((prev) => !prev)}
                aria-expanded={!searchResultsCollapsed}
              >
                {searchResultsCollapsed
                  ? `Show results (${searchResults.length})`
                  : `Hide results (${searchResults.length})`}
              </button>

              {!searchResultsCollapsed && (
                <ul ref={searchResultsListRef} className="space-y-2 max-h-80 overflow-y-auto pr-1">
                  {searchResults.map((result) => {
                    const queued = isQueueBlocked(result.video_id);
                    return (
                      <li key={result.video_id} className="tile rounded-xl px-2 py-2">
                        <div className="flex items-start gap-3">
                          <Image
                            src={result.thumbnail_url}
                            alt={result.title}
                            width={112}
                            height={64}
                            unoptimized
                            className="h-16 w-28 rounded-md border border-white/10 object-cover"
                            loading="lazy"
                          />
                          <div className="min-w-0 flex-1">
                            <p className="text-sm font-medium leading-5 line-clamp-2">{result.title}</p>
                            <p className="text-xs muted mt-1 truncate">{result.channel}</p>
                            <p className="text-[11px] muted mt-1 font-mono">{result.video_id}</p>
                          </div>
                          <div className="flex shrink-0 self-center items-center gap-2">
                            {canControl && (
                              <button
                                type="button"
                                className="btn-primary px-3 py-2 text-xs"
                                onClick={() => {
                                  setQueueMetaById((prev) => ({ ...prev, [result.video_id]: result }));
                                  submitVideoId(result.video_id, 'load', 'search_result');
                                }}
                              >
                                Load
                              </button>
                            )}
                            {canQueue && (
                              <button
                                type="button"
                                className="btn-secondary px-3 py-2 text-xs"
                                onClick={() => {
                                  setQueueMetaById((prev) => ({ ...prev, [result.video_id]: result }));
                                  submitVideoId(result.video_id, 'queue', 'search_result');
                                }}
                                disabled={queued || !wsConnected}
                                title={
                                  queued
                                    ? 'Already queued (or currently playing)'
                                    : 'Add this video to play next'
                                }
                              >
                                {queued ? 'Queued' : 'Queue'}
                              </button>
                            )}
                          </div>
                        </div>
                      </li>
                    );
                  })}
                </ul>
              )}
            </div>
          )}
        </div>
      )}

      {queue.length > 0 && (
        <div className="panel-soft rounded-xl px-3 py-3">
          <p className="mb-2 text-xs uppercase tracking-wide muted">
            Up Next ({queue.length})
          </p>
          <ul className="space-y-1 text-xs">
            {queue.map((videoId, idx) => (
              <li
                key={`${videoId}-${idx}`}
                data-youtube-queue-index={idx}
                className="tile rounded-lg px-2 py-1.5"
              >
                <div className="flex items-start gap-2">
                  {queueMetaById[videoId] ? (
                    <div className="min-w-0 flex-1 space-y-0.5">
                      {canControl ? (
                        <button
                          type="button"
                          className="text-left text-xs hover:underline"
                          onClick={() => requestQueuePlayNow(idx)}
                          title="Play this queued video now"
                        >
                          {idx + 1}. {queueMetaById[videoId].title}
                        </button>
                      ) : (
                        <p className="text-xs">
                          {idx + 1}. {queueMetaById[videoId].title}
                        </p>
                      )}
                      <p className="text-[11px] muted truncate">
                        {queueMetaById[videoId].channel}
                      </p>
                    </div>
                  ) : (
                    <div className="min-w-0 flex-1">
                      <span className="font-mono">{idx + 1}. {videoId}</span>
                    </div>
                  )}
                  <div className="flex shrink-0 self-center items-center gap-1">
                    {canControl && (
                      <button
                        type="button"
                        className="btn-primary px-2 py-1 text-[11px]"
                        onClick={() => requestQueuePlayNow(idx)}
                        disabled={!wsConnected}
                        title="Play this video immediately"
                      >
                        Play now
                      </button>
                    )}
                    {canQueue && (
                      <>
                        <button
                          type="button"
                          className="btn-secondary px-2 py-1 text-[11px]"
                          onClick={() => requestQueueMove(idx, idx - 1)}
                          disabled={!wsConnected || idx === 0}
                          title="Move up"
                        >
                          Up
                        </button>
                        <button
                          type="button"
                          className="btn-secondary px-2 py-1 text-[11px]"
                          onClick={() => requestQueueMove(idx, idx + 1)}
                          disabled={!wsConnected || idx === queue.length - 1}
                          title="Move down"
                        >
                          Down
                        </button>
                        <button
                          type="button"
                          className="btn-secondary px-2 py-1 text-[11px]"
                          onClick={() => requestQueueRemove(idx)}
                          disabled={!wsConnected}
                          title="Remove from queue"
                        >
                          Remove
                        </button>
                      </>
                    )}
                  </div>
                </div>
              </li>
            ))}
          </ul>
        </div>
      )}

      {playerError && (
        <p className="text-xs text-red-300">{playerError}</p>
      )}

      {!wsConnected && (
        <p className="text-xs text-red-300">
          Realtime connection is disconnected. Video load/play commands will not sync until reconnected.
        </p>
      )}

      {!canControl && (
        <p className="text-xs muted">
          You are not an admin — only admins can play/pause/seek or load the current video.
        </p>
      )}
    </section>
  );
}
