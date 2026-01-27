'use client'

import { Panels, PanelsToolbar } from '@/components/Panels'
import { Workspace, StatusBar } from '@/components/Canvas'
import { Navigator } from '@/components/Navigator'
import { ActivityBubble } from '@/components/ActivityBubble'

export default function Page() {
  return (
    <div className='flex min-h-0 flex-1 flex-col'>
      <ActivityBubble />
      <div className='flex min-h-0 flex-1'>
        <Navigator />
        <div className='flex min-h-0 flex-1 flex-col overflow-hidden'>
          <Workspace />
          <StatusBar />
        </div>
        <PanelsToolbar />
        <Panels />
      </div>
    </div>
  )
}
