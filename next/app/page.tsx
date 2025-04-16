'use client'

import DetectionPanel from '@/components/detection-panel'
import Tools from '@/components/tools'
import Topbar from '@/components/topbar'
import Canvas from '@/components/canvas'

function App() {
  return (
    <main className='flex flex-col h-screen w-screen max-h-screen max-w-screen bg-gray-100'>
      <Topbar />
      <div className='flex flex-1 my-2'>
        <Tools />

        <div className='flex-1 flex-col overflow-auto'>
          <Canvas />
        </div>

        <div className='h-full overflow-y-auto'>
          <DetectionPanel />
        </div>
      </div>
    </main>
  )
}

export default App
