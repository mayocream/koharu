'use client'

import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ChevronDownIcon, LayersIcon, ALargeSmallIcon } from 'lucide-react'
import { LayersPanel } from '@/components/panels/LayersPanel'
import { TextBlocksPanel } from '@/components/panels/TextBlocksPanel'
import { RenderControls } from '@/components/canvas/CanvasToolbar'
import { ResizableSidebar } from '@/components/ResizableSidebar'
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '@/components/ui/popover'
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import { cn } from '@/lib/utils'

export function PanelsToolbar() {
  const { t } = useTranslation()

  return (
    <div className='border-border bg-card flex w-11 shrink-0 flex-col items-center gap-1 border-l py-2'>
      <Popover>
        <Tooltip>
          <TooltipTrigger asChild>
            <PopoverTrigger asChild>
              <button className='text-muted-foreground hover:text-foreground hover:bg-accent flex h-8 w-8 items-center justify-center rounded'>
                <ALargeSmallIcon className='size-4' />
              </button>
            </PopoverTrigger>
          </TooltipTrigger>
          <TooltipContent side='left' sideOffset={8}>
            {t('render.fontLabel')}
          </TooltipContent>
        </Tooltip>
        <PopoverContent side='left' align='start' className='w-auto p-3'>
          <RenderControls />
        </PopoverContent>
      </Popover>
    </div>
  )
}

export function Panels() {
  const { t } = useTranslation()
  const [layersExpanded, setLayersExpanded] = useState(true)

  return (
    <ResizableSidebar
      side='right'
      initialWidth={280}
      minWidth={200}
      maxWidth={400}
      className='border-border bg-muted/50 border-l'
    >
      <div className='flex h-full w-full flex-col'>
        {/* Layers Section */}
        <div className='flex flex-col'>
          <button
            onClick={() => setLayersExpanded(!layersExpanded)}
            className='hover:bg-accent/50 border-border flex w-full items-center gap-1.5 border-b px-2 py-1.5 text-left'
          >
            <ChevronDownIcon
              className={cn(
                'text-muted-foreground size-3.5 transition-transform',
                !layersExpanded && '-rotate-90',
              )}
            />
            <LayersIcon className='size-3.5' />
            <span className='text-xs font-semibold tracking-wide uppercase'>
              {t('layers.title')}
            </span>
          </button>
          {layersExpanded && (
            <div className='border-border border-b'>
              <LayersPanel />
            </div>
          )}
        </div>

        {/* Text Blocks Section - takes remaining space */}
        <TextBlocksPanel />
      </div>
    </ResizableSidebar>
  )
}
