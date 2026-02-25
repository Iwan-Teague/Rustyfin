'use client';

import { useEffect, useMemo, useState } from 'react';
import { WsWebStateMessage } from '@/lib/watchPartyApi';

type Props = {
  roomId: string;
  webState: WsWebStateMessage | null;
  canControl: boolean;
  wsConnected: boolean;
  sendWs: (payload: Record<string, unknown>) => boolean;
};

function normalizeWebInput(raw: string): string | null {
  const trimmed = raw.trim();
  if (!trimmed) return null;

  const noScheme = trimmed
    .replace(/^https?:\/\//i, '')
    .replace(/^\/\//, '')
    .trim();
  if (!noScheme) return null;

  const candidate = `https://${noScheme}`;

  try {
    const parsed = new URL(candidate);
    if (parsed.protocol !== 'https:') {
      return null;
    }
    if (!parsed.host) return null;
    return parsed.toString();
  } catch {
    return null;
  }
}

function toInputHostPath(url: string): string {
  const trimmed = url.trim();
  if (!trimmed) return '';
  try {
    const parsed = new URL(trimmed);
    const path = parsed.pathname || '/';
    return `${parsed.host}${path}${parsed.search}${parsed.hash}`;
  } catch {
    return trimmed.replace(/^https?:\/\//i, '').replace(/^\/\//, '');
  }
}

export default function WebPlayer({ roomId, webState, canControl, wsConnected, sendWs }: Props) {
  const [urlInput, setUrlInput] = useState('');
  const [error, setError] = useState('');
  const [isEditingUrl, setIsEditingUrl] = useState(false);

  const activeUrl = useMemo(() => webState?.url?.trim() || '', [webState?.url]);
  const activeHost = useMemo(() => {
    if (!activeUrl) return '';
    try {
      return new URL(activeUrl).hostname.toLowerCase();
    } catch {
      return '';
    }
  }, [activeUrl]);

  useEffect(() => {
    if (!activeUrl) return;
    if (isEditingUrl) return;
    const nextInput = toInputHostPath(activeUrl);
    setUrlInput((current) => (current === nextInput ? current : nextInput));
  }, [activeUrl, isEditingUrl]);

  const submitUrl = () => {
    setError('');
    setIsEditingUrl(false);

    if (!canControl) {
      setError('Only room admins can change the shared web URL.');
      return;
    }
    if (!wsConnected) {
      setError('Realtime connection is offline. Reconnect and retry.');
      return;
    }

    const normalized = normalizeWebInput(urlInput);
    if (!normalized) {
      setError('Enter a valid website (for example: google.com or mozilla.org/firefox).');
      return;
    }

    const sent = sendWs({ type: 'change_web_url', url: normalized });
    if (!sent) {
      setError('Failed to send URL update. Reconnect and retry.');
      return;
    }

    setUrlInput(toInputHostPath(normalized));
  };

  return (
    <section className="space-y-4">
      <div className="panel-soft rounded-xl px-3 py-3 space-y-3">
        <div className="flex flex-col gap-2 sm:flex-row">
          <div className={`flex flex-1 items-center overflow-hidden rounded-lg border border-white/10 bg-black/20 ${!canControl ? 'opacity-70' : ''}`}>
            <span className="select-none border-r border-white/10 px-3 py-2 text-sm muted">https://</span>
            <input
              type="text"
              value={urlInput}
              onChange={(e) => setUrlInput(e.target.value.replace(/^https?:\/\//i, '').replace(/^\/\//, ''))}
              onFocus={() => setIsEditingUrl(true)}
              onBlur={() => setIsEditingUrl(false)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') {
                  e.preventDefault();
                  submitUrl();
                }
              }}
              placeholder="www.mozilla.org/"
              className="w-full bg-transparent px-3 py-2 text-sm outline-none"
              maxLength={2048}
              disabled={!canControl}
              aria-label="Shared website (host and path)"
            />
          </div>
          <button
            type="button"
            className="btn-primary px-4 py-2 text-sm disabled:opacity-50"
            onClick={submitUrl}
            disabled={!canControl || !wsConnected}
          >
            Open
          </button>
          <a
            href={activeUrl || '#'}
            target="_blank"
            rel="noopener noreferrer"
            className={`btn-secondary px-4 py-2 text-sm text-center ${
              activeUrl ? '' : 'pointer-events-none opacity-50'
            }`}
          >
            Open in new tab
          </a>
        </div>
        {!canControl && (
          <p className="text-xs muted">
            You are a member in this room. Only admins can change the shared web page.
          </p>
        )}
        {error && <p className="text-xs text-red-300">{error}</p>}
        {activeHost.endsWith('google.com') && (
          <p className="text-xs text-yellow-200">
            Google blocks iframe embedding. Use a different site in the room view, or click Open in new tab.
          </p>
        )}
      </div>

      <div className="tile overflow-hidden rounded-2xl border border-white/10 bg-black relative">
        {activeUrl ? (
          <iframe
            key={`${roomId}:${activeUrl}`}
            src={activeUrl}
            title="Shared web view"
            className="h-[82vh] min-h-[640px] w-full"
            allow="autoplay; fullscreen; picture-in-picture; encrypted-media"
            referrerPolicy="strict-origin-when-cross-origin"
          />
        ) : (
          <div className="h-[82vh] min-h-[640px] flex items-center justify-center px-6 text-center">
            <p className="text-sm muted">
              No shared page loaded yet. An admin can enter a URL above to start.
            </p>
          </div>
        )}
      </div>

      <p className="text-xs muted">
        Some sites block iframe embedding with security headers. If a page appears blank, use a different site or open it in a new tab.
      </p>
    </section>
  );
}
