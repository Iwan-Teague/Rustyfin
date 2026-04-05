import { NextResponse } from 'next/server';

type RuntimeConfigResponse = {
  backend_origin: string | null;
  ice_servers: RuntimeIceServerResponse[];
};

type RuntimeIceServerResponse = {
  urls: string[];
  username?: string | null;
  credential?: string | null;
};

function normalizeHttpOrigin(raw: string | undefined): string | null {
  if (!raw) return null;
  const trimmed = raw.trim();
  if (!trimmed) return null;

  try {
    const url = new URL(trimmed);
    if (url.protocol !== 'http:' && url.protocol !== 'https:') {
      return null;
    }
    return `${url.protocol}//${url.host}`;
  } catch {
    return null;
  }
}

function normalizeIceUrls(raw: unknown): string[] {
  if (Array.isArray(raw)) {
    return raw
      .flatMap((value) => (typeof value === 'string' ? [value] : []))
      .map((value) => value.trim())
      .filter((value) => value.length > 0);
  }
  if (typeof raw === 'string') {
    return raw
      .split(',')
      .map((value) => value.trim())
      .filter((value) => value.length > 0);
  }
  return [];
}

function sanitizeIceServer(raw: unknown): RuntimeIceServerResponse | null {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) {
    return null;
  }
  const record = raw as Record<string, unknown>;
  const urls = normalizeIceUrls(record.urls);
  if (urls.length === 0) {
    return null;
  }
  const username =
    typeof record.username === 'string' && record.username.trim().length > 0
      ? record.username.trim()
      : null;
  const credential =
    typeof record.credential === 'string' && record.credential.trim().length > 0
      ? record.credential.trim()
      : null;
  return {
    urls,
    username,
    credential,
  };
}

function parseConfiguredIceServers(): RuntimeIceServerResponse[] {
  const rawJson = process.env.RUSTFIN_WEBRTC_ICE_SERVERS_JSON?.trim();
  if (rawJson) {
    try {
      const parsed = JSON.parse(rawJson) as unknown;
      const candidates = Array.isArray(parsed) ? parsed : [parsed];
      const servers = candidates
        .map((candidate) => sanitizeIceServer(candidate))
        .filter((value): value is RuntimeIceServerResponse => value !== null);
      if (servers.length > 0) {
        return servers;
      }
    } catch {
      // fall through to env-based assembly
    }
  }

  const servers: RuntimeIceServerResponse[] = [];
  const stunUrls = normalizeIceUrls(
    process.env.RUSTFIN_WEBRTC_STUN_URL ?? process.env.NEXT_PUBLIC_STUN_URL ?? 'stun:stun.l.google.com:19302',
  );
  if (stunUrls.length > 0) {
    servers.push({ urls: stunUrls });
  }

  const turnUrls = normalizeIceUrls(
    process.env.RUSTFIN_WEBRTC_TURN_URLS ?? process.env.RUSTFIN_WEBRTC_TURN_URL,
  );
  if (turnUrls.length > 0) {
    servers.push({
      urls: turnUrls,
      username:
        process.env.RUSTFIN_WEBRTC_TURN_USERNAME?.trim() ||
        null,
      credential:
        process.env.RUSTFIN_WEBRTC_TURN_CREDENTIAL?.trim() ||
        null,
    });
  }

  return servers;
}

export async function GET() {
  const backendOrigin =
    normalizeHttpOrigin(process.env.RUSTYFIN_BROWSER_BACKEND_ORIGIN) ||
    normalizeHttpOrigin(process.env.RUSTYFIN_API_BASE_URL) ||
    null;

  const payload: RuntimeConfigResponse = {
    backend_origin: backendOrigin,
    ice_servers: parseConfiguredIceServers(),
  };

  return NextResponse.json(payload, {
    headers: {
      'Cache-Control': 'no-store',
    },
  });
}
