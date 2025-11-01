import type { NextConfig } from 'next'

const nextConfig: NextConfig = {
  reactCompiler: true,
  devIndicators: false,
  output: 'export',
  images: {
    unoptimized: true,
  },
}

export default nextConfig
