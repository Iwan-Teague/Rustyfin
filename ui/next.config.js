const apiBaseUrl = process.env.RUSTYFIN_API_BASE_URL || 'http://localhost:8096';

/** @type {import('next').NextConfig} */
const nextConfig = {
  output: 'standalone',
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
};

module.exports = nextConfig;
