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
