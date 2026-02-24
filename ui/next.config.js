const apiBaseUrl = process.env.RUSTYFIN_API_BASE_URL || 'http://localhost:8096';

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
