'use client'

import { useEffect, useState } from 'react'
import * as detection from '@/inference/detection'
import * as ocr from '@/inference/ocr'
import * as inpaint from '@/inference/inpaint'
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
        // TODO: Handle error more gracefully
        alert(
          `Failed to initialize the model. Please check your internet connection or try again later.\n\n${error}`
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
