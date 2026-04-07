import type { NextConfig } from 'next'

const nextConfig: NextConfig = {
  reactCompiler: true,
  devIndicators: false,
  output: 'export',
  images: {
    unoptimized: true,
  },
  experimental: {
    proxyClientMaxBodySize: '1gb',
    proxyTimeout: 300000,
  },
  async rewrites() {
    return [
      {
        source: '/api/v1/:path*',
        destination: 'http://127.0.0.1:9999/api/v1/:path*',
      },
    ]
  },
}

export default nextConfig
