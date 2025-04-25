'use client'

import DetectionPanel from '@/components/detection-panel'
import Tools from '@/components/tools'
import Topbar from '@/components/topbar'
import Canvas from '@/components/canvas'
import OCRPanel from '@/components/ocr-panel'
import { useWorkflowStore } from '@/lib/state'
import TranslationPanel from '@/components/translation-panel'

function App() {
  const { selectedTool } = useWorkflowStore()

  return (
    <main className='flex h-screen max-h-screen w-screen max-w-screen flex-col bg-gray-100'>
      <Topbar />
      <div className='flex flex-1'>
        <div className='fixed z-50 mx-3 mt-13 pt-3'>
          <Tools />
        </div>

        <div className='flex h-screen flex-1 flex-col items-center justify-center'>
          <Canvas />
        </div>

        <div className='fixed right-3 mt-13 ml-3 flex h-[calc(100vh-3.5rem)] flex-col gap-2 overflow-y-auto pt-3'>
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
