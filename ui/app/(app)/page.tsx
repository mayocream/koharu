'use client'

import { Panels } from '@/components/Panels'
import { Workspace, StatusBar } from '@/components/Canvas'
import { Navigator } from '@/components/Navigator'
import { ActivityBubble } from '@/components/ActivityBubble'
import { AppInitializationSkeleton } from '@/components/AppInitializationSkeleton'
import { useGetMeta } from '@/lib/api/system/system'
import { useKeyboardNavigation } from '@/hooks/useKeyboardNavigation'
import { usePrefetchProcessing } from '@/hooks/usePrefetchProcessing'
import { usePrefetchAdjacentBlobs } from '@/hooks/usePrefetchAdjacentBlobs'
import { useBackgroundProcessing } from '@/hooks/useBackgroundProcessing'
import {
  Group,
  Panel,
  Separator,
  useDefaultLayout,
} from 'react-resizable-panels'
import { AppErrorBoundary } from '@/components/AppErrorBoundary'

const LAYOUT_ID = 'koharu-main-layout-v2'

export default function Page() {
  // Enable keyboard navigation (left/right arrows to navigate pages)
  useKeyboardNavigation()
  // Prefetch detection/OCR for nearby pages as user navigates
  usePrefetchProcessing()
  // Prefetch adjacent page blobs for faster navigation
  usePrefetchAdjacentBlobs()
  // Background processing: Detect → OCR for all pages
  useBackgroundProcessing()

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

  if (!meta) {
    return <AppInitializationSkeleton />
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
        <Panel id='right' defaultSize={280} minSize={260} maxSize={400}>
          <AppErrorBoundary>
            <Panels />
          </AppErrorBoundary>
        </Panel>
      </Group>
    </div>
  )
}
