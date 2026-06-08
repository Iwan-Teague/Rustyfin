import { NextRequest, NextResponse } from 'next/server';

type PublicSystemInfo = {
  setup_completed: boolean;
};

async function parseResponseBody(res: Response): Promise<unknown> {
  if (res.status === 204 || res.status === 205 || res.status === 304) {
    return undefined;
  }

  const raw = await res.text();
  const trimmed = raw.trim();
  if (!trimmed) {
    return undefined;
  }

  const contentType = res.headers.get('content-type') || '';
  const looksJson =
    contentType.includes('application/json') ||
    contentType.includes('+json') ||
    trimmed.startsWith('{') ||
    trimmed.startsWith('[');

  if (!looksJson) {
    return trimmed;
  }

  try {
    return JSON.parse(trimmed);
  } catch {
    return trimmed;
  }
}

async function getSetupCompleted(request: NextRequest): Promise<boolean | null> {
  const url = new URL('/api/v1/system/info/public', request.url);

  try {
    const res = await fetch(url, {
      method: 'GET',
      cache: 'no-store',
      headers: {
        Accept: 'application/json',
      },
    });

    if (!res.ok) {
      return null;
    }

    const info = (await parseResponseBody(res)) as PublicSystemInfo;
    if (!info || typeof info !== 'object' || typeof info.setup_completed !== 'boolean') {
      return null;
    }
    return info.setup_completed;
  } catch {
    return null;
  }
}

function isVaultRoute(pathname: string): boolean {
  return pathname === '/vault' || pathname.startsWith('/vault/');
}

function applyVaultHeaders(response: NextResponse): NextResponse {
  // RustyVault's portable Argon2id path loads a same-origin Wasm glue module
  // (`/vendor/rustyvault/argon2-browser.js`) via dynamic import() and runs it
  // through WebAssembly.instantiate(Streaming). That needs `'wasm-unsafe-eval'`
  // but NOT the broad `'unsafe-eval'` — the glue contains no eval()/new Function()
  // (SEC-6). `'unsafe-inline'` is retained because this is a Next.js App Router
  // page whose SSR document includes unnonced inline hydration/bootstrap scripts
  // (e.g. self.__next_f.push(...)); dropping it without wiring Next's nonce
  // propagation would break hydration and leave the unlock form non-interactive.
  const scriptSrc = "script-src 'self' 'unsafe-inline' 'wasm-unsafe-eval'";
  const csp = [
    "default-src 'self'",
    "base-uri 'self'",
    "frame-ancestors 'none'",
    "object-src 'none'",
    scriptSrc,
    "style-src 'self' 'unsafe-inline'",
    "img-src 'self' data: blob:",
    "font-src 'self' data:",
    "connect-src 'self'",
    "worker-src 'self' blob:",
    "frame-src 'none'",
    "form-action 'self'",
  ].join('; ');

  response.headers.set('Cache-Control', 'no-store, max-age=0, must-revalidate');
  response.headers.set('Pragma', 'no-cache');
  response.headers.set('Expires', '0');
  response.headers.set('Referrer-Policy', 'no-referrer');
  response.headers.set('X-Content-Type-Options', 'nosniff');
  response.headers.set('X-Frame-Options', 'DENY');
  response.headers.set('Permissions-Policy', 'camera=(), microphone=(), geolocation=(), payment=(), usb=(), serial=()');
  response.headers.set('Content-Security-Policy', csp);
  return response;
}

function applyBaseHeaders(response: NextResponse): NextResponse {
  // Baseline security headers for every non-vault app route (SEC-5). The vault
  // route stays stricter via applyVaultHeaders; the two sets intentionally differ.
  //
  // This CSP is deliberately pragmatic, not maximal: the data plane (API, HLS
  // playlists/segments, range streams, artwork, avatars) is all same-origin
  // (Next.js rewrites proxy /api and /stream to the backend), so 'self' covers
  // it — but several first-class social features need wider allowances and a
  // CSP that blocked them would be worse than a permissive one.
  const csp = [
    "default-src 'self'",
    "base-uri 'self'",
    // Clickjacking protection: Rustyfin itself must never be framed. Pairs with
    // the X-Frame-Options: DENY header below for older-browser coverage.
    "frame-ancestors 'none'",
    "object-src 'none'",
    // 'unsafe-inline': Next.js App Router emits unnonced inline hydration/bootstrap
    //   scripts; without nonce propagation a strict script-src breaks every page.
    // youtube.com: the watch-together rooms inject the YouTube IFrame API script.
    "script-src 'self' 'unsafe-inline' https://www.youtube.com",
    // Tailwind + Next inject inline <style>; styled via 'self' otherwise.
    "style-src 'self' 'unsafe-inline'",
    // 'self' = artwork/avatars (/api/v1/.../images, /api/v1/users/avatar/...);
    // data:/blob: = generated posters and client-side previews;
    // https: = YouTube thumbnails (ytimg) and any externally-hosted poster art.
    "img-src 'self' data: blob: https:",
    "font-src 'self' data:",
    // 'self' = same-origin API + HLS manifest/segment fetches (hls.js) + range stream.
    // ws:/wss: = channels voice socket and watch-party room socket, including the
    //   operator-configured direct-backend fallback origin from /runtime-config.
    // https: = /runtime-config external lookups and YouTube player data XHR.
    "connect-src 'self' ws: wss: https:",
    // 'self' = direct range streams (/stream/file/...); blob: = hls.js MSE source.
    "media-src 'self' blob:",
    // hls.js runs with enableWorker:true, spawning its demux worker from a blob: URL.
    "worker-src 'self' blob:",
    // Browse-together (WebPlayer) iframes arbitrary user-entered sites, and the
    // YouTube watch-together embeds youtube.com — so child frames need broad
    // http/https. (frame-ancestors above still blocks Rustyfin being embedded.)
    "frame-src 'self' https: http:",
    "form-action 'self'",
  ].join('; ');

  response.headers.set('Referrer-Policy', 'strict-origin-when-cross-origin');
  response.headers.set('X-Content-Type-Options', 'nosniff');
  response.headers.set('X-Frame-Options', 'DENY');
  response.headers.set(
    'Permissions-Policy',
    // Keep camera/microphone/display-capture self-enabled: voice channels, screen
    // share and webcam rooms rely on getUserMedia/getDisplayMedia on this origin.
    'geolocation=(), payment=(), usb=(), serial=(), camera=(self), microphone=(self), display-capture=(self)',
  );
  response.headers.set('Content-Security-Policy', csp);
  return response;
}

function applySecurityHeaders(response: NextResponse, vaultRoute: boolean): NextResponse {
  return vaultRoute ? applyVaultHeaders(response) : applyBaseHeaders(response);
}

export async function middleware(request: NextRequest) {
  const { pathname } = request.nextUrl;
  const vaultRoute = isVaultRoute(pathname);
  if (pathname === '/health') {
    return applyBaseHeaders(NextResponse.next());
  }
  const isSetupRoute = pathname === '/setup' || pathname.startsWith('/setup/');
  const isLoginRoute = pathname === '/login';
  const authToken = request.cookies.get('rustfin_token')?.value;

  const setupCompleted = await getSetupCompleted(request);
  if (setupCompleted === null) {
    const response = NextResponse.next();
    return applySecurityHeaders(response, vaultRoute);
  }

  if (!setupCompleted && !isSetupRoute) {
    const response = NextResponse.redirect(new URL('/setup', request.url));
    return applySecurityHeaders(response, vaultRoute);
  }

  if (setupCompleted && isSetupRoute) {
    const response = NextResponse.redirect(new URL('/', request.url));
    return applySecurityHeaders(response, vaultRoute);
  }

  if (setupCompleted && !isSetupRoute && !isLoginRoute && !authToken) {
    const response = NextResponse.redirect(new URL('/login', request.url));
    return applySecurityHeaders(response, vaultRoute);
  }

  const response = NextResponse.next();
  return applySecurityHeaders(response, vaultRoute);
}

export const config = {
  matcher: [
    '/((?!api|stream|_next/static|_next/image|favicon.ico|robots.txt|sitemap.xml|.*\\..*).*)',
  ],
};
