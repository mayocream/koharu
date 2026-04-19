import { withSentryConfig } from '@sentry/nextjs'
import type { NextConfig } from 'next'

const nextConfig: NextConfig = {
  reactCompiler: true,
  devIndicators: false,
  output: 'export',
  // Next's dev server otherwise gzip-compresses proxied responses; zlib
  // buffers small `text/event-stream` chunks until its internal window
  // fills, which never happens for low-volume SSE → the UI sees the
  // connection open but no frames arrive. Browsers can't opt out via
  // `Accept-Encoding: identity` (it's a forbidden request header), so
  // this has to be a server-side switch. Safe in prod: `output: 'export'`
  // means the Rust backend serves the static UI directly — Next's
  // compression layer is only in the picture during `next dev`.
  compress: false,
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

export default withSentryConfig(nextConfig, {
  org: 'koharu-d0',
  project: 'nextjs',
  silent: !process.env.CI,
})
