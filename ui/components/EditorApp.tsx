'use client'

import { useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import {
  Group,
  Panel,
  Separator,
  useDefaultLayout,
  type PanelImperativeHandle,
} from 'react-resizable-panels'

import { ActivityBubble } from '@/components/ActivityBubble'
import { StatusBar } from '@/components/canvas/StatusBar'
import { Workspace } from '@/components/canvas/Workspace'
import { MenuBar } from '@/components/MenuBar'
import { Navigator } from '@/components/Navigator'
import { Panels } from '@/components/Panels'
import { SettingsDialog } from '@/components/SettingsDialog'
import { WelcomeScreen } from '@/components/WelcomeScreen'
import { useEditorStore } from '@/lib/koharu'
import { cn } from '@/lib/utils'

const LAYOUT_ID = 'koharu-main-layout-v3'

export function EditorApp() {
  const { t } = useTranslation()
  const connection = useEditorStore((state) => state.connection)
  const project = useEditorStore((state) => state.project)
  const error = useEditorStore((state) => state.error)
  const notice = useEditorStore((state) => state.notice)
  const setError = useEditorStore((state) => state.setError)
  const setNotice = useEditorStore((state) => state.setNotice)
  const showNavigator = useEditorStore((state) => state.showNavigator)
  const setShowNavigator = useEditorStore((state) => state.setShowNavigator)
  const navigatorRef = useRef<PanelImperativeHandle>(null)
  const { defaultLayout, onLayoutChanged } = useDefaultLayout({
    id: LAYOUT_ID,
    panelIds: ['left', 'center', 'right'],
  })

  useEffect(() => {
    if (!notice) return
    const timeout = window.setTimeout(() => setNotice(null), 5000)
    return () => window.clearTimeout(timeout)
  }, [notice, setNotice])

  useEffect(() => {
    const panel = navigatorRef.current
    if (!panel) return
    if (showNavigator && panel.isCollapsed()) panel.expand()
    else if (!showNavigator && !panel.isCollapsed()) panel.collapse()
  }, [showNavigator])

  return (
    <div className='flex h-screen w-screen flex-col overflow-hidden bg-transparent'>
      <MenuBar />
      {error && (
        <button
          className='border-b border-destructive/30 bg-destructive/10 px-3 py-1.5 text-left text-xs text-destructive'
          onClick={() => setError(null)}
        >
          {error}
        </button>
      )}
      {notice && (
        <button
          className='border-b border-primary/20 bg-primary/10 px-3 py-1.5 text-left text-xs'
          onClick={() => setNotice(null)}
        >
          {notice}
        </button>
      )}
      {connection === 'connecting' ? (
        <main className='grid min-h-0 flex-1 place-items-center bg-background text-sm text-muted-foreground'>
          {t('common.initializing')}
        </main>
      ) : !project ? (
        <WelcomeScreen disconnected={connection === 'disconnected'} />
      ) : (
        <>
          <ActivityBubble />
          <Group
            orientation='horizontal'
            id={LAYOUT_ID}
            defaultLayout={defaultLayout}
            onLayoutChanged={onLayoutChanged}
            className='flex min-h-0 flex-1'
          >
            <Panel
              panelRef={navigatorRef}
              id='left'
              defaultSize={160}
              minSize={160}
              maxSize={250}
              collapsible
              collapsedSize={0}
              onResize={(size) => {
                if (size.asPercentage === 0 && showNavigator) setShowNavigator(false)
                else if (size.asPercentage > 0 && !showNavigator) setShowNavigator(true)
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
              <div className='flex h-full min-h-0 min-w-0 flex-1 flex-col overflow-hidden'>
                <Workspace />
                <StatusBar />
              </div>
            </Panel>
            <Separator className='w-px bg-border transition-colors hover:bg-border' />
            <Panel id='right' defaultSize={280} minSize={280} maxSize={400}>
              <Panels />
            </Panel>
          </Group>
        </>
      )}
      <SettingsDialog />
    </div>
  )
}
