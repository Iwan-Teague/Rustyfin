'use client';

import { createContext, useContext, useRef, useState, useCallback, useEffect, type ReactNode } from 'react';
import { apiJson } from '@/lib/api';

export interface MusicTrack {
  id: string;
  title: string;
  artist: string;
  albumTitle: string;
  albumArtUrl?: string;
  durationMs?: number;
}

interface PlaybackDescriptor {
  direct_url: string;
}

interface MusicPlayerContextValue {
  queue: MusicTrack[];
  currentIndex: number;
  playing: boolean;
  progress: number;   // seconds
  duration: number;   // seconds
  volume: number;
  currentTrack: MusicTrack | null;
  playQueue: (tracks: MusicTrack[], startIndex?: number) => void;
  playPause: () => void;
  seek: (seconds: number) => void;
  next: () => void;
  prev: () => void;
  setVolume: (v: number) => void;
  stop: () => void;
}

const MusicPlayerContext = createContext<MusicPlayerContextValue | null>(null);

export function MusicPlayerProvider({ children }: { children: ReactNode }) {
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const [queue, setQueue] = useState<MusicTrack[]>([]);
  const [currentIndex, setCurrentIndex] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [progress, setProgress] = useState(0);
  const [duration, setDuration] = useState(0);
  const [volume, setVolumeState] = useState(1);

  // Create hidden audio element once
  useEffect(() => {
    const audio = new Audio();
    audio.volume = 1;
    audioRef.current = audio;

    audio.addEventListener('timeupdate', () => setProgress(audio.currentTime));
    audio.addEventListener('durationchange', () => setDuration(audio.duration || 0));
    audio.addEventListener('ended', () => {
      setCurrentIndex((idx) => idx + 1);
    });
    audio.addEventListener('play', () => setPlaying(true));
    audio.addEventListener('pause', () => setPlaying(false));

    return () => {
      audio.pause();
      audio.src = '';
    };
  }, []);

  // Load and play when currentIndex or queue changes
  useEffect(() => {
    if (queue.length === 0) return;
    if (currentIndex >= queue.length) {
      // Reached end of queue
      setPlaying(false);
      setCurrentIndex(0);
      return;
    }
    const track = queue[currentIndex];
    const audio = audioRef.current;
    if (!audio) return;

    apiJson<PlaybackDescriptor>(`/items/${track.id}/playback`)
      .then((desc) => {
        audio.src = desc.direct_url;
        audio.play().catch(() => {});
      })
      .catch(() => {});
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [currentIndex, queue]);

  const playQueue = useCallback((tracks: MusicTrack[], startIndex = 0) => {
    setQueue(tracks);
    setCurrentIndex(startIndex);
    setProgress(0);
    setDuration(0);
  }, []);

  const playPause = useCallback(() => {
    const audio = audioRef.current;
    if (!audio) return;
    if (audio.paused) {
      audio.play().catch(() => {});
    } else {
      audio.pause();
    }
  }, []);

  const seek = useCallback((seconds: number) => {
    const audio = audioRef.current;
    if (audio) {
      audio.currentTime = seconds;
      setProgress(seconds);
    }
  }, []);

  const next = useCallback(() => {
    setCurrentIndex((idx) => Math.min(idx + 1, queue.length - 1));
    setProgress(0);
  }, [queue.length]);

  const prev = useCallback(() => {
    const audio = audioRef.current;
    // If more than 3 seconds in, restart current track; otherwise go back
    if (audio && audio.currentTime > 3) {
      audio.currentTime = 0;
      setProgress(0);
    } else {
      setCurrentIndex((idx) => Math.max(idx - 1, 0));
      setProgress(0);
    }
  }, []);

  const setVolume = useCallback((v: number) => {
    const audio = audioRef.current;
    if (audio) audio.volume = v;
    setVolumeState(v);
  }, []);

  const stop = useCallback(() => {
    const audio = audioRef.current;
    if (audio) {
      audio.pause();
      audio.src = '';
    }
    setQueue([]);
    setCurrentIndex(0);
    setProgress(0);
    setDuration(0);
    setPlaying(false);
  }, []);

  const currentTrack = queue.length > 0 && currentIndex < queue.length
    ? queue[currentIndex]
    : null;

  return (
    <MusicPlayerContext.Provider
      value={{
        queue,
        currentIndex,
        playing,
        progress,
        duration,
        volume,
        currentTrack,
        playQueue,
        playPause,
        seek,
        next,
        prev,
        setVolume,
        stop,
      }}
    >
      {children}
    </MusicPlayerContext.Provider>
  );
}

export function useMusicPlayer() {
  const ctx = useContext(MusicPlayerContext);
  if (!ctx) throw new Error('useMusicPlayer must be used inside MusicPlayerProvider');
  return ctx;
}
