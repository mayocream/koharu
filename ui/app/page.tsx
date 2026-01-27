'use client'

import { MenuBar } from '@/components/MenuBar'
import { Panels } from '@/components/Panels'
import { Workspace, StatusBar } from '@/components/Canvas'
import { Navigator } from '@/components/Navigator'
import { ActivityBubble } from '@/components/ActivityBubble'

export default function Page() {
  return (
    <main className='relative flex h-screen w-screen flex-col overflow-hidden bg-neutral-50'>
      <MenuBar />
      <ActivityBubble />
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
