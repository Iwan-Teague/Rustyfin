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

const DEFAULT_TURN_PORT = 3478;

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

function normalizeHostCandidate(raw: string | null | undefined): string | null {
  if (!raw) return null;
  const trimmed = raw
    .split(',')
    .map((value) => value.trim())
    .find((value) => value.length > 0);
  if (!trimmed) return null;

  try {
    const url = new URL(trimmed.includes('://') ? trimmed : `https://${trimmed}`);
    return url.hostname;
  } catch {
    return null;
  }
}

function shouldExposeTurnHost(host: string): boolean {
  const normalized = host.trim().toLowerCase();
  if (!normalized) return false;
  return (
    normalized !== 'localhost' &&
    normalized !== '127.0.0.1' &&
    normalized !== '0.0.0.0' &&
    normalized !== '::1' &&
    normalized !== '[::1]'
  );
}

function dedupeUrls(urls: string[]): string[] {
  const seen = new Set<string>();
  const deduped: string[] = [];
  for (const value of urls) {
    if (seen.has(value)) continue;
    seen.add(value);
    deduped.push(value);
  }
  return deduped;
}

function buildDerivedTurnUrls(host: string): string[] {
  return [
    `turn:${host}:${DEFAULT_TURN_PORT}?transport=udp`,
    `turn:${host}:${DEFAULT_TURN_PORT}?transport=tcp`,
  ];
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

function withDerivedTurnHosts(
  servers: RuntimeIceServerResponse[],
  request: Request,
): RuntimeIceServerResponse[] {
  const turnUsername = process.env.RUSTFIN_WEBRTC_TURN_USERNAME?.trim() || null;
  const turnCredential = process.env.RUSTFIN_WEBRTC_TURN_CREDENTIAL?.trim() || null;
  if (!turnUsername || !turnCredential) {
    return servers;
  }

  const hostCandidates = [
    normalizeHostCandidate(request.headers.get('x-forwarded-host')),
    normalizeHostCandidate(request.headers.get('host')),
    normalizeHostCandidate(process.env.RUSTFIN_PUBLIC_HOST),
  ].filter((value): value is string => Boolean(value && shouldExposeTurnHost(value)));

  if (hostCandidates.length === 0) {
    return servers;
  }

  const derivedUrls = dedupeUrls(hostCandidates.flatMap((host) => buildDerivedTurnUrls(host)));
  if (derivedUrls.length === 0) {
    return servers;
  }

  const nextServers = [...servers];
  const existingTurnIndex = nextServers.findIndex(
    (server) =>
      server.username === turnUsername &&
      server.credential === turnCredential &&
      server.urls.some((value) => value.startsWith('turn:')),
  );

  if (existingTurnIndex >= 0) {
    nextServers[existingTurnIndex] = {
      ...nextServers[existingTurnIndex],
      urls: dedupeUrls([...nextServers[existingTurnIndex].urls, ...derivedUrls]),
    };
    return nextServers;
  }

  nextServers.push({
    urls: derivedUrls,
    username: turnUsername,
    credential: turnCredential,
  });
  return nextServers;
}

export async function GET(request: Request) {
  const backendOrigin =
    normalizeHttpOrigin(process.env.RUSTYFIN_BROWSER_BACKEND_ORIGIN) ||
    normalizeHttpOrigin(process.env.RUSTYFIN_API_BASE_URL) ||
    null;

  const payload: RuntimeConfigResponse = {
    backend_origin: backendOrigin,
    ice_servers: withDerivedTurnHosts(parseConfiguredIceServers(), request),
  };

  return NextResponse.json(payload, {
    headers: {
      'Cache-Control': 'no-store',
    },
  });
}
