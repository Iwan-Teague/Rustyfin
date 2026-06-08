const backendPort = process.env.RUSTFIN_BACKEND_PORT || '8097';
const calendarPort = process.env.RUSTFIN_CALENDAR_PORT || '8099';
const apiBaseUrl = process.env.RUSTYFIN_API_BASE_URL || `http://127.0.0.1:${backendPort}`;
const calendarApiBaseUrl =
  process.env.RUSTYFIN_CALENDAR_API_BASE_URL || `http://127.0.0.1:${calendarPort}`;

/** @type {import('next').NextConfig} */
const nextConfig = {
  output: 'standalone',
  eslint: {
    // Linting runs in dedicated CI/test scripts; keep production build resilient.
    ignoreDuringBuilds: true,
  },
  async headers() {
    // Defense-in-depth floor applied to EVERY response, including the static
    // assets, _next/* chunks and proxied routes that src/middleware.ts excludes
    // from its matcher. Deliberately limited to headers that are always safe on
    // JS/CSS/images/JSON so they cannot break asset delivery or media streaming.
    // The full Content-Security-Policy (and the route-specific RustyVault
    // hardening) is set per-document in src/middleware.ts, not here, so static
    // assets are never burdened with a CSP they don't need.
    return [
      {
        source: '/:path*',
        headers: [
          { key: 'X-Content-Type-Options', value: 'nosniff' },
          { key: 'X-Frame-Options', value: 'DENY' },
          { key: 'Referrer-Policy', value: 'strict-origin-when-cross-origin' },
        ],
      },
    ];
  },
  async rewrites() {
    return [
      {
        source: '/api/v1/calendar/:path*',
        destination: `${calendarApiBaseUrl}/api/v1/calendar/:path*`,
      },
      {
        source: '/api/:path*',
        destination: `${apiBaseUrl}/api/:path*`,
      },
      {
        source: '/stream/:path*',
        destination: `${apiBaseUrl}/stream/:path*`,
      },
      {
        source: '/health',
        destination: `${apiBaseUrl}/health`,
      },
    ];
  },
  async redirects() {
    return [
      {
        source: '/watch-party',
        destination: '/rooms',
        permanent: true,
      },
      {
        source: '/watch-party/rooms/:roomId',
        destination: '/rooms/:roomId',
        permanent: true,
      },
    ];
  },
};

module.exports = nextConfig;
