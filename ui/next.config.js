/** @type {import('next').NextConfig} */
const nextConfig = {
  output: 'standalone',
  async rewrites() {
    return [
      {
        source: '/api/:path*',
        destination: 'http://localhost:8096/api/:path*',
      },
      {
        source: '/stream/:path*',
        destination: 'http://localhost:8096/stream/:path*',
      },
      {
        source: '/health',
        destination: 'http://localhost:8096/health',
      },
    ];
  },
};

module.exports = nextConfig;
