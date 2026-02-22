'use client'

import { Panels, PanelsToolbar } from '@/components/Panels'
import { Workspace, StatusBar } from '@/components/Canvas'
import { Navigator } from '@/components/Navigator'
import { ActivityBubble } from '@/components/ActivityBubble'
import {
  Group,
  Panel,
  Separator,
  useDefaultLayout,
} from 'react-resizable-panels'
import { AppErrorBoundary } from '@/components/AppErrorBoundary'

const LAYOUT_ID = 'koharu-main-layout-v2'

export default function Page() {
  const { defaultLayout, onLayoutChanged } = useDefaultLayout({
    id: LAYOUT_ID,
    panelIds: ['left', 'center', 'right'],
  })

  return (
    <div className='flex min-h-0 flex-1 flex-col'>
      <ActivityBubble />
      <Group
        orientation='horizontal'
        id={LAYOUT_ID}
        defaultLayout={defaultLayout}
        onLayoutChanged={onLayoutChanged}
        className='flex min-h-0 flex-1'
      >
        <Panel id='left' defaultSize={220} minSize={160} maxSize={360}>
          <Navigator />
        </Panel>
        <Separator className='bg-border/40 hover:bg-border w-1 transition-colors' />
        <Panel id='center' minSize={480}>
          <AppErrorBoundary>
            <div className='flex h-full min-h-0 min-w-0 flex-1 flex-col overflow-hidden'>
              <Workspace />
              <StatusBar />
            </div>
          </AppErrorBoundary>
        </Panel>
        <Separator className='bg-border/40 hover:bg-border w-1 transition-colors' />
        <Panel id='right' defaultSize={320} minSize={320} maxSize={460}>
          <AppErrorBoundary>
            <div className='flex h-full min-h-0'>
              <PanelsToolbar />
              <Panels />
            </div>
          </AppErrorBoundary>
        </Panel>
      </Group>
    </div>
  )
}
