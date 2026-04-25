'use client'

import { useEffect, useRef } from 'react'
import { Group, Panel, Separator, useDefaultLayout, type PanelImperativeHandle } from 'react-resizable-panels'

import { ActivityBubble } from '@/components/ActivityBubble'
import { AppErrorBoundary } from '@/components/AppErrorBoundary'
import { AppInitializationSkeleton } from '@/components/AppInitializationSkeleton'
import { Workspace, StatusBar } from '@/components/Canvas'
import { Navigator } from '@/components/Navigator'
import { Panels } from '@/components/Panels'
import { WelcomeScreen } from '@/components/WelcomeScreen'
import { useScene } from '@/hooks/useScene'
import { useGetMeta } from '@/lib/api/default/default'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { cn } from '@/lib/utils'

const LAYOUT_ID = 'koharu-main-layout-v3'

export default function Page() {
  const hasProject = useScene().scene !== null
  const showNavigator = useEditorUiStore((s) => s.showNavigator)
  const setShowNavigator = useEditorUiStore((s) => s.setShowNavigator)
  const leftPanelRef = useRef<PanelImperativeHandle>(null)

  const { defaultLayout, onLayoutChanged } = useDefaultLayout({
    id: LAYOUT_ID,
    panelIds: ['left', 'center', 'right'],
  })

  // Sync store -> panel state
  useEffect(() => {
    const panel = leftPanelRef.current
    if (!panel) return

    if (showNavigator && panel.isCollapsed()) {
      panel.expand()
    } else if (!showNavigator && !panel.isCollapsed()) {
      panel.collapse()
    }
  }, [showNavigator])

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
        <Panel
          panelRef={leftPanelRef}
          id='left'
          defaultSize={160}
          minSize={160}
          maxSize={250}
          collapsible
          collapsedSize={0}
          onResize={(size) => {
            if (size.asPercentage === 0 && showNavigator) {
              setShowNavigator(false)
            } else if (size.asPercentage > 0 && !showNavigator) {
              setShowNavigator(true)
            }
          }}
        >
          <Navigator />
        </Panel>
        <Separator
          className={cn(
            'w-px bg-border transition-colors hover:bg-border',
            !showNavigator && 'hidden',
          )}
        />
        <Panel id='center' minSize={480}>
          <AppErrorBoundary>
            <div className='flex h-full min-h-0 min-w-0 flex-1 flex-col overflow-hidden'>
              <Workspace />
              <StatusBar />
            </div>
          </AppErrorBoundary>
        </Panel>
        <Separator className='w-px bg-border transition-colors hover:bg-border' />
        <Panel id='right' defaultSize={280} minSize={280} maxSize={400}>
          <AppErrorBoundary>
            <Panels />
          </AppErrorBoundary>
        </Panel>
      </Group>
    </div>
  )
}
