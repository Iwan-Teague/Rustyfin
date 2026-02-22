import { NextResponse } from 'next/server';

type RuntimeConfigResponse = {
  backend_origin: string | null;
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

export async function GET() {
  const backendOrigin =
    normalizeHttpOrigin(process.env.RUSTYFIN_BROWSER_BACKEND_ORIGIN) ||
    normalizeHttpOrigin(process.env.RUSTYFIN_API_BASE_URL) ||
    null;

  const payload: RuntimeConfigResponse = {
    backend_origin: backendOrigin,
  };

  return NextResponse.json(payload, {
    headers: {
      'Cache-Control': 'no-store',
    },
  });
}
