import { MenuBar } from '@/components/MenuBar'
import { Panels } from '@/components/Panels'
import { Canvas, CanvasControl } from '@/components/Canvas'
import { ThumbnailPanel } from '@/components/ThumbnailPanel'

export default function Page() {
  return (
    <main className='flex flex-1 flex-col h-screen w-screen overflow-hidden'>
      <MenuBar />
      <div className='flex min-h-0 min-w-0 flex-1'>
        <ThumbnailPanel />
        <div className='flex min-h-0 min-w-0 flex-1 flex-col'>
          <Canvas />
          <CanvasControl />
        </div>
        <Panels />
      </div>
    </main>
  )
}
