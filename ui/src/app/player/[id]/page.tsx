'use client';

import { useEffect, useRef, useState } from 'react';
import { useParams } from 'next/navigation';
import { apiJson, apiFetch } from '@/lib/api';

export default function PlayerPage() {
  const params = useParams();
  const id = params.id as string;
  const videoRef = useRef<HTMLVideoElement>(null);
  const [mode, setMode] = useState<'direct' | 'hls'>('direct');
  const [fileId, setFileId] = useState<string | null>(null);
  const [hlsUrl, setHlsUrl] = useState<string | null>(null);
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [error, setError] = useState('');

  // Get the file ID for this item
  useEffect(() => {
    // Items have a file_id field from the episode_file_map
    apiJson<{ file_id?: string }>(`/items/${id}`)
      .then((item: any) => {
        if (item.file_id) {
          setFileId(item.file_id);
        }
      })
      .catch((e) => setError(e.message));
  }, [id]);

  // Set up direct play URL when file ID is available
  useEffect(() => {
    if (!fileId || !videoRef.current) return;

    if (mode === 'direct') {
      videoRef.current.src = `/stream/file/${fileId}`;
    }
  }, [fileId, mode]);

  // Start HLS session
  async function startHls() {
    if (!fileId) return;

    try {
      const data = await apiJson<{ session_id: string; hls_url: string }>(
        '/playback/sessions',
        {
          method: 'POST',
          body: JSON.stringify({ file_id: fileId }),
        }
      );
      setSessionId(data.session_id);
      setHlsUrl(data.hls_url);
      setMode('hls');

      // Use hls.js if available, otherwise native HLS
      const video = videoRef.current;
      if (!video) return;

      if (video.canPlayType('application/vnd.apple.mpegurl')) {
        // Safari native HLS
        video.src = data.hls_url;
      } else {
        // Use hls.js for Chrome/Firefox
        const Hls = (await import('hls.js')).default;
        if (Hls.isSupported()) {
          const hls = new Hls();
          hls.loadSource(data.hls_url);
          hls.attachMedia(video);
        }
      }
    } catch (e: any) {
      setError(e.message || 'Failed to start HLS');
    }
  }

  // Report progress periodically
  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;

    const interval = setInterval(() => {
      if (video.currentTime > 0) {
        apiFetch('/playback/progress', {
          method: 'POST',
          body: JSON.stringify({
            item_id: id,
            progress_ms: Math.floor(video.currentTime * 1000),
            played: video.ended,
          }),
        }).catch(() => {});
      }
    }, 10000);

    return () => clearInterval(interval);
  }, [id]);

  // Cleanup HLS session on unmount
  useEffect(() => {
    return () => {
      if (sessionId) {
        apiFetch(`/playback/sessions/${sessionId}/stop`, { method: 'POST' }).catch(() => {});
      }
    };
  }, [sessionId]);

  return (
    <div className="space-y-4">
      {error && <p className="text-red-400">{error}</p>}

      <div className="bg-black rounded-lg overflow-hidden">
        <video
          ref={videoRef}
          controls
          autoPlay
          className="w-full max-h-[80vh]"
          playsInline
        />
      </div>

      <div className="flex gap-3">
        <button
          onClick={() => setMode('direct')}
          className={`px-4 py-2 rounded text-sm font-medium transition ${
            mode === 'direct'
              ? 'bg-blue-600 text-white'
              : 'bg-gray-800 text-gray-400 hover:text-white'
          }`}
        >
          Direct Play
        </button>
        <button
          onClick={startHls}
          className={`px-4 py-2 rounded text-sm font-medium transition ${
            mode === 'hls'
              ? 'bg-blue-600 text-white'
              : 'bg-gray-800 text-gray-400 hover:text-white'
          }`}
        >
          Transcode (HLS)
        </button>
      </div>
    </div>
  );
}
