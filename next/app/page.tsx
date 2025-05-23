'use client'

import DetectionPanel from '@/components/detection-panel'
import Tools from '@/components/tools'
import Topbar from '@/components/topbar'
import Canvas from '@/components/canvas'
import OCRPanel from '@/components/ocr-panel'
import { useWorkflowStore } from '@/lib/state'
import TranslationPanel from '@/components/translation-panel'
import SplashScreen from '@/components/splashscreen'
import { useEffect, useState } from 'react'
import * as detection from '@/lib/detection'
import * as ocr from '@/lib/ocr'
import * as inpaint from '@/lib/inpaint'

function App() {
  const [progress, setProgress] = useState(0)
  const [loading, setLoading] = useState(true)
  const { selectedTool } = useWorkflowStore()

  useEffect(() => {
    const initialize = async () => {
      await detection.initialize().then(() => setProgress(30))
      await ocr.initialize().then(() => setProgress(70))
      await inpaint.initialize().then(() => setProgress(100))
    }
    initialize().then(() => {
      setLoading(false)
    })
  }, [])

  if (loading) {
    return <SplashScreen progress={progress} />
  }

  return (
    <main className='flex h-screen max-h-screen w-screen max-w-screen flex-col bg-gray-100'>
      <Topbar />
      <div className='flex flex-1 overflow-hidden'>
        <div className='flex h-full w-20 items-start p-3'>
          <Tools />
        </div>

        <div className='flex flex-1 flex-col items-center justify-center'>
          <Canvas />
        </div>

        <div className='flex h-full w-72 flex-col gap-2 overflow-y-auto p-3'>
          {selectedTool === 'detection' && (
            <>
              <DetectionPanel />
              <OCRPanel />
            </>
          )}
          {selectedTool === 'translation' && <TranslationPanel />}
        </div>
      </div>
    </main>
  )
}

export default App
