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
    <main className='flex flex-col h-screen w-screen max-h-screen max-w-screen bg-gray-100'>
      <Topbar />
      <div className='flex flex-1 my-2'>
        <Tools />

        <div className='flex-1 flex-col overflow-auto'>
          <Canvas />
        </div>

        <div className='flex flex-col h-full overflow-y-auto gap-2'>
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
