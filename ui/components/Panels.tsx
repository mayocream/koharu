'use client'

import { useTranslation } from 'react-i18next'
import { ProcessingControls } from '@/components/panels/ProcessingControls'
import { LlmControls } from '@/components/panels/LlmControls'
import { RenderControls } from '@/components/panels/RenderControls'
import { TextBlocksPanel } from '@/components/panels/TextBlocksPanel'
import { ResizableSidebar } from '@/components/ResizableSidebar'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'

const PANEL_TABS = [
  {
    value: 'processing',
    labelKey: 'panels.processing',
    Component: ProcessingControls,
  },
  { value: 'llm', labelKey: 'panels.llm', Component: LlmControls },
  { value: 'render', labelKey: 'panels.render', Component: RenderControls },
] as const

export function Panels() {
  const { t } = useTranslation()

  return (
    <ResizableSidebar
      side='right'
      initialWidth={256}
      minWidth={220}
      maxWidth={420}
      className='border-border bg-muted border-l'
    >
      <div className='flex h-full w-full flex-col'>
        <Tabs defaultValue='processing' className='border-border border-b'>
          <TabsList className='bg-card text-muted-foreground grid w-full grid-cols-3 text-[11px] font-semibold tracking-wide uppercase'>
            {PANEL_TABS.map((tab) => (
              <TabsTrigger
                key={tab.value}
                value={tab.value}
                className='data-[state=active]:bg-accent rounded-none px-2.5 py-1.5'
              >
                {t(tab.labelKey)}
              </TabsTrigger>
            ))}
          </TabsList>
          <div className='px-2.5 py-2'>
            {PANEL_TABS.map(({ value, Component }) => (
              <TabsContent key={value} value={value}>
                <Component />
              </TabsContent>
            ))}
          </div>
        </Tabs>
        <TextBlocksPanel />
      </div>
    </ResizableSidebar>
  )
}
