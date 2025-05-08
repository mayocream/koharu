import type { NextConfig } from 'next'

const isProd = process.env.NODE_ENV === 'production'
const internalHost = process.env.TAURI_DEV_HOST || 'localhost'

const nextConfig: NextConfig = {
  output: 'export',
  images: {
    unoptimized: true,
  },
  distDir: './dist',
  assetPrefix: isProd ? undefined : `http://${internalHost}:9000`,
  reactStrictMode: false,
}

export default nextConfig
