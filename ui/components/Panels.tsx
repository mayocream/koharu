'use client'

import { useTranslation } from 'react-i18next'
import { LayersIcon, SlidersHorizontalIcon } from 'lucide-react'
import { LayersPanel } from '@/components/panels/LayersPanel'
import { TextBlocksPanel } from '@/components/panels/TextBlocksPanel'
import { RenderControls } from '@/components/canvas/CanvasToolbar'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { ScrollArea } from '@/components/ui/scroll-area'

export function Panels() {
  const { t } = useTranslation()

  return (
    <div className='bg-muted/50 flex h-full min-h-0 w-full flex-col border-l'>
      <Tabs
        defaultValue='layers'
        className='border-border h-60 shrink-0 gap-0 border-b'
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
          className='min-h-0 flex-1 px-1 pb-2 data-[state=inactive]:hidden'
          data-testid='panels-layers'
        >
          <ScrollArea className='h-full' viewportClassName='pr-1'>
            <LayersPanel />
          </ScrollArea>
        </TabsContent>

        <TabsContent
          value='layout'
          className='min-h-0 flex-1 px-2 pb-2 data-[state=inactive]:hidden'
          data-testid='panels-layout'
        >
          <ScrollArea className='h-full' viewportClassName='pr-1'>
            <div className='pt-1'>
              <RenderControls />
            </div>
          </ScrollArea>
        </TabsContent>
      </Tabs>

      {/* Text Blocks Section - takes remaining space */}
      <TextBlocksPanel />
    </div>
  )
}
