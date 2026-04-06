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
  // RustyVault's portable Argon2id path uses a browser-loaded Wasm module.
  const scriptSrc = "script-src 'self' 'unsafe-inline' 'unsafe-eval' 'wasm-unsafe-eval'";
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

export async function middleware(request: NextRequest) {
  const { pathname } = request.nextUrl;
  const vaultRoute = isVaultRoute(pathname);
  if (pathname === '/health') {
    return NextResponse.next();
  }
  const isSetupRoute = pathname === '/setup' || pathname.startsWith('/setup/');
  const isLoginRoute = pathname === '/login';
  const authToken = request.cookies.get('rustfin_token')?.value;

  const setupCompleted = await getSetupCompleted(request);
  if (setupCompleted === null) {
    const response = NextResponse.next();
    return vaultRoute ? applyVaultHeaders(response) : response;
  }

  if (!setupCompleted && !isSetupRoute) {
    const response = NextResponse.redirect(new URL('/setup', request.url));
    return vaultRoute ? applyVaultHeaders(response) : response;
  }

  if (setupCompleted && isSetupRoute) {
    const response = NextResponse.redirect(new URL('/', request.url));
    return vaultRoute ? applyVaultHeaders(response) : response;
  }

  if (setupCompleted && !isSetupRoute && !isLoginRoute && !authToken) {
    const response = NextResponse.redirect(new URL('/login', request.url));
    return vaultRoute ? applyVaultHeaders(response) : response;
  }

  const response = NextResponse.next();
  return vaultRoute ? applyVaultHeaders(response) : response;
}

export const config = {
  matcher: [
    '/((?!api|stream|_next/static|_next/image|favicon.ico|robots.txt|sitemap.xml|.*\\..*).*)',
  ],
};
