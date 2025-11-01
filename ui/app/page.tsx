import { MenuBar } from '@/components/MenuBar'
import { Panels } from '@/components/Panels'
import { Canvas, CanvasControl } from '@/components/Canvas'
import { ThumbnailPanel } from '@/components/ThumbnailPanel'

export default function Page() {
  return (
    <main className='flex min-h-0 flex-1 flex-col'>
      <MenuBar />
      <div className='flex min-h-0 flex-1'>
        <aside>
          <ThumbnailPanel />
        </aside>
        <section className='flex min-h-0 flex-1 flex-col'>
          <Canvas />
          <div className='px-2 pb-2'>
            <CanvasControl />
          </div>
        </section>
        <aside>
          <Panels />
        </aside>
      </div>
    </main>
  )
}
