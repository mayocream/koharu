'use client'

import { useTranslation } from 'react-i18next'
import { LayersIcon, SlidersHorizontalIcon } from 'lucide-react'
import { LayersPanel } from '@/components/panels/LayersPanel'
import { RenderControlsPanel } from '@/components/panels/RenderControlsPanel'
import { TextBlocksPanel } from '@/components/panels/TextBlocksPanel'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'

export function Panels() {
  const { t } = useTranslation()

  return (
    <div className='bg-muted/50 flex h-full min-h-0 w-full flex-col border-l'>
      <Tabs
        defaultValue='layers'
        className='border-border shrink-0 gap-0 border-b'
        data-testid='panels-settings-tabs'
      >
        <TabsList className='bg-muted/70 m-2 mb-0 grid w-[calc(100%-1rem)] grid-cols-2'>
          <TabsTrigger
            value='layers'
            data-testid='panels-tab-layers'
            className='gap-1'
          >
            <LayersIcon className='size-3.5' />
            <span className='text-xs font-semibold tracking-wide uppercase'>
              {t('layers.title')}
            </span>
          </TabsTrigger>
          <TabsTrigger
            value='layout'
            data-testid='panels-tab-layout'
            className='gap-1'
          >
            <SlidersHorizontalIcon className='size-3.5' />
            <span className='text-xs font-semibold tracking-wide uppercase'>
              {t('panels.render')}
            </span>
          </TabsTrigger>
        </TabsList>

        <TabsContent
          value='layers'
          className='px-1 pb-2 data-[state=inactive]:hidden'
          data-testid='panels-layers'
        >
          <LayersPanel />
        </TabsContent>

        <TabsContent
          value='layout'
          className='px-2 pt-1 pb-2 data-[state=inactive]:hidden'
          data-testid='panels-layout'
        >
          <RenderControlsPanel />
        </TabsContent>
      </Tabs>

      {/* Text Blocks Section - takes remaining space */}
      <TextBlocksPanel />
    </div>
  )
}
