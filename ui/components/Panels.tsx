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
      className='border-l border-neutral-200 bg-neutral-50'
    >
      <div className='flex h-full w-full flex-col'>
        <Tabs defaultValue='processing' className='border-b border-neutral-200'>
          <TabsList className='grid w-full grid-cols-3 bg-white text-[11px] font-semibold tracking-wide text-neutral-600 uppercase'>
            {PANEL_TABS.map((tab) => (
              <TabsTrigger
                key={tab.value}
                value={tab.value}
                className='rounded-none px-2.5 py-1.5 data-[state=active]:bg-neutral-100'
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
