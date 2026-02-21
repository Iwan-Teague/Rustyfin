import { NextRequest, NextResponse } from 'next/server';

type PublicSystemInfo = {
  setup_completed: boolean;
};

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

    const info = (await res.json()) as PublicSystemInfo;
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

  return NextResponse.next();
}

export const config = {
  matcher: [
    '/((?!api|stream|_next/static|_next/image|favicon.ico|robots.txt|sitemap.xml|.*\\..*).*)',
  ],
};
