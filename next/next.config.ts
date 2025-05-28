import type { NextConfig } from 'next'

const nextConfig: NextConfig = {
  distDir: 'dist',
  output: 'export',
  reactStrictMode: false,
  // refer: https://web.dev/articles/cross-origin-isolation-guide
  async headers() {
    return [
      {
        source: '/:path*',
        headers: [
          {
            key: 'Cross-Origin-Opener-Policy',
            value: 'same-origin',
          },
          {
            key: 'Cross-Origin-Embedder-Policy',
            value: 'require-corp',
          },
        ],
      },
    ]
  },
}

export default nextConfig
