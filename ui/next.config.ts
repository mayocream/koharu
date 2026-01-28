import type { NextConfig } from 'next'

const isProd = process.env.NODE_ENV === 'production'
const backendPort = process.env.BACKEND_PORT || '8080'

const nextConfig: NextConfig = {
  reactCompiler: true,
  devIndicators: false,
  // Static export only for production build
  output: isProd ? 'export' : undefined,
  images: {
    unoptimized: true,
  },
  // Proxy API requests to backend in dev mode
  async rewrites() {
    if (isProd) return []
    return [
      {
        source: '/api/:path*',
        destination: `http://127.0.0.1:${backendPort}/api/:path*`,
      },
    ]
  },
}

export default nextConfig
