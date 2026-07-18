'use client'

import { LayersIcon, SlidersHorizontalIcon, SparklesIcon, TypeIcon } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { AiPanel } from '@/components/panels/AiPanel'
import { LayersPanel } from '@/components/panels/LayersPanel'
import { RenderControlsPanel } from '@/components/panels/RenderControlsPanel'
import { TextBlocksPanel } from '@/components/panels/TextBlocksPanel'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { useGetCodexAuthStatus } from '@/lib/api/default/default'

export function Panels() {
  const { t } = useTranslation()
  const { data: codexAuth } = useGetCodexAuthStatus()
  const codexSignedIn = codexAuth?.signedIn === true
  const [workTab, setWorkTab] = useState('text')

  useEffect(() => {
    if (!codexSignedIn && workTab === 'ai') setWorkTab('text')
  }, [codexSignedIn, workTab])

  return (
    <div className='flex h-full min-h-0 w-full flex-col border-l bg-muted/50'>
      <Tabs
        defaultValue='layers'
        className='shrink-0 gap-0 border-b border-border'
        data-testid='panels-settings-tabs'
      >
        <TabsList className='m-2 mb-0 grid w-[calc(100%-1rem)] grid-cols-2 bg-muted/70'>
          <TabsTrigger value='layers' data-testid='panels-tab-layers' className='gap-1'>
            <LayersIcon className='size-3.5' />
            <span className='text-xs font-semibold tracking-wide uppercase'>
              {t('layers.title')}
            </span>
          </TabsTrigger>
          <TabsTrigger value='layout' data-testid='panels-tab-layout' className='gap-1'>
            <SlidersHorizontalIcon className='size-3.5' />
            <span className='text-xs font-semibold tracking-wide uppercase'>
              {t('panels.render')}
            </span>
          </TabsTrigger>
        </TabsList>

        <TabsContent
          value='layers'
          className='overflow-y-auto max-h-60 px-1 pb-2 data-[state=inactive]:hidden'
          data-testid='panels-layers'
        >
          <LayersPanel />
        </TabsContent>

        <TabsContent
          value='layout'
          className='overflow-y-auto max-h-60 px-2 pb-2 data-[state=inactive]:hidden'
          data-testid='panels-layout'
        >
          <div className='pt-1'>
            <RenderControlsPanel />
          </div>
        </TabsContent>
      </Tabs>

      <Tabs
        value={workTab}
        onValueChange={setWorkTab}
        className='min-h-0 flex-1 gap-0'
        data-testid='panels-work-tabs'
      >
        {codexSignedIn && (
          <TabsList className='m-2 mb-0 grid w-[calc(100%-1rem)] grid-cols-2 bg-muted/70'>
            <TabsTrigger value='text' data-testid='panels-tab-textblocks' className='gap-1'>
              <TypeIcon className='size-3.5' />
              <span className='text-xs font-semibold tracking-wide uppercase'>
                {t('layers.textBlocks')}
              </span>
            </TabsTrigger>
            <TabsTrigger value='ai' data-testid='panels-tab-ai' className='gap-1'>
              <SparklesIcon className='size-3.5' />
              <span className='text-xs font-semibold tracking-wide uppercase'>
                {t('panels.ai')}
              </span>
            </TabsTrigger>
          </TabsList>
        )}

        <TabsContent
          value='text'
          className='flex min-h-0 flex-1 data-[state=inactive]:hidden'
          data-testid='panels-textblocks-tab'
        >
          <TextBlocksPanel />
        </TabsContent>

        {codexSignedIn && (
          <TabsContent
            value='ai'
            className='min-h-0 flex-1 px-2 pb-2 data-[state=inactive]:hidden'
            data-testid='panels-ai'
          >
            <ScrollArea className='h-full' viewportClassName='pr-1 [&>div]:!block'>
              <div className='pt-2'>
                <AiPanel />
              </div>
            </ScrollArea>
          </TabsContent>
        )}
      </Tabs>
    </div>
  )
}
