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
      <div className='my-2 mt-13 flex flex-1'>
        <div className='mx-3 mt-4'>
          <Tools />
        </div>

        <div className='flex-1 flex-col overflow-auto py-4'>
          <Canvas />
        </div>

        <div className='mx-3 mt-4 flex h-full flex-col gap-2 overflow-y-auto'>
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
