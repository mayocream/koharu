'use client'

import { useEffect, useState } from 'react'
import * as detection from '@/lib/detection'
import * as ocr from '@/lib/ocr'
import * as inpaint from '@/lib/inpaint'
import SplashScreen from '@/components/splashscreen'

export default function Layout({
  children,
}: Readonly<{
  children: React.ReactNode
}>) {
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    const initialize = async () => {
      try {
        await detection.initialize()
        await ocr.initialize()
        await inpaint.initialize()

        console.log('All models initialized successfully')
      } catch (error) {
        alert(
          `Failed to initialize the model. Please check your internet connection or try again later.\n\nError: ${error}`
        )
      } finally {
        setLoading(false)
      }
    }

    initialize()
  }, [])

  if (loading) {
    return <SplashScreen />
  }

  return <>{children}</>
}
