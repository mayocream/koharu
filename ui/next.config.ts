import type { NextConfig } from 'next'

const isProd = process.env.NODE_ENV === 'production'
const internalHost = process.env.TAURI_DEV_HOST || 'localhost'

const nextConfig: NextConfig = {
  reactCompiler: true,
  devIndicators: false,
  output: 'export',
  images: {
    unoptimized: true,
  },
  // https://v2.tauri.app/start/frontend/nextjs/#update-nextjs-configuration
  assetPrefix: isProd ? undefined : `http://${internalHost}:3000`,
}

export default nextConfig
