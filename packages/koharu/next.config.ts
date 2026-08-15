import type { NextConfig } from 'next'

const nextConfig: NextConfig = {
  devIndicators: false,
  transpilePackages: ['@koharu/bridge', '@koharu/ui'],
  output: 'export',
  images: {
    unoptimized: true,
  },
}

export default nextConfig
