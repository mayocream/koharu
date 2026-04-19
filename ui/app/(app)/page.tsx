'use client'

import { Group, Panel, Separator, useDefaultLayout } from 'react-resizable-panels'

import { ActivityBubble } from '@/components/ActivityBubble'
import { AppErrorBoundary } from '@/components/AppErrorBoundary'
import { AppInitializationSkeleton } from '@/components/AppInitializationSkeleton'
import { Workspace, StatusBar } from '@/components/Canvas'
import { Navigator } from '@/components/Navigator'
import { Panels } from '@/components/Panels'
import { WelcomeScreen } from '@/components/WelcomeScreen'
import { useScene } from '@/hooks/useScene'
import { useGetMeta } from '@/lib/api/default/default'

const LAYOUT_ID = 'koharu-main-layout-v2'

export default function Page() {
  const { defaultLayout, onLayoutChanged } = useDefaultLayout({
    id: LAYOUT_ID,
    panelIds: ['left', 'center', 'right'],
  })
  const { data: meta } = useGetMeta({
    query: {
      retry: false,
      refetchInterval: (query) => (query.state.data ? false : 1500),
      staleTime: Infinity,
    },
  })
  const hasProject = useScene().scene !== null

  if (!meta) {
    return <AppInitializationSkeleton />
  }

  if (!hasProject) {
    return <WelcomeScreen />
  }

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
        <Panel id='left' defaultSize={180} minSize={120} maxSize={300}>
          <Navigator />
        </Panel>
        <Separator className='w-1 bg-border/40 transition-colors hover:bg-border' />
        <Panel id='center' minSize={480}>
          <AppErrorBoundary>
            <div className='flex h-full min-h-0 min-w-0 flex-1 flex-col overflow-hidden'>
              <Workspace />
              <StatusBar />
            </div>
          </AppErrorBoundary>
        </Panel>
        <Separator className='w-1 bg-border/40 transition-colors hover:bg-border' />
        <Panel id='right' defaultSize={280} minSize={260} maxSize={400}>
          <AppErrorBoundary>
            <Panels />
          </AppErrorBoundary>
        </Panel>
      </Group>
    </div>
  )
}
