import { MenuBar } from '@/components/MenuBar'
import { Panels } from '@/components/Panels'
import { Workspace, StatusBar } from '@/components/Canvas'
import { Navigator } from '@/components/Navigator'
import { UpdateBubble } from '@/components/UpdateBubble'

export default function Page() {
  return (
    <main className='relative flex h-screen w-screen flex-col overflow-hidden bg-neutral-50'>
      <MenuBar />
      <UpdateBubble />
      <div className='flex min-h-0 flex-1'>
        <Navigator />
        <div className='flex min-h-0 flex-1 flex-col'>
          <Workspace />
          <StatusBar />
        </div>
        <Panels />
      </div>
    </main>
  )
}
