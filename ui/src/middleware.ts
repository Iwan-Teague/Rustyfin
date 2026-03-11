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

export async function middleware(request: NextRequest) {
  const { pathname } = request.nextUrl;
  if (pathname === '/health') {
    return NextResponse.next();
  }
  const isSetupRoute = pathname === '/setup' || pathname.startsWith('/setup/');
  const isLoginRoute = pathname === '/login';
  const authToken = request.cookies.get('rustfin_token')?.value;

  const setupCompleted = await getSetupCompleted(request);
  if (setupCompleted === null) {
    return NextResponse.next();
  }

  if (!setupCompleted && !isSetupRoute) {
    return NextResponse.redirect(new URL('/setup', request.url));
  }

  if (setupCompleted && isSetupRoute) {
    return NextResponse.redirect(new URL('/', request.url));
  }

  if (setupCompleted && !isSetupRoute && !isLoginRoute && !authToken) {
    return NextResponse.redirect(new URL('/login', request.url));
  }

  if (setupCompleted && isLoginRoute && authToken) {
    return NextResponse.redirect(new URL('/', request.url));
  }

  return NextResponse.next();
}

export const config = {
  matcher: [
    '/((?!api|stream|_next/static|_next/image|favicon.ico|robots.txt|sitemap.xml|.*\\..*).*)',
  ],
};
