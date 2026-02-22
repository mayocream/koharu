'use client'

import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { motion, AnimatePresence } from 'motion/react'
import { ChevronDownIcon, LayersIcon, ALargeSmallIcon } from 'lucide-react'
import { LayersPanel } from '@/components/panels/LayersPanel'
import { TextBlocksPanel } from '@/components/panels/TextBlocksPanel'
import { RenderControls } from '@/components/canvas/CanvasToolbar'
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
import { Button } from '@/components/ui/button'

export function PanelsToolbar() {
  const { t } = useTranslation()

  return (
    <div className='border-border bg-card flex w-11 shrink-0 flex-col items-center gap-1 border-l py-2'>
      <Popover>
        <Tooltip>
          <TooltipTrigger asChild>
            <PopoverTrigger asChild>
              <Button
                variant='ghost'
                size='icon-sm'
                className='text-muted-foreground'
              >
                <ALargeSmallIcon className='size-4' />
              </Button>
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
    <div className='bg-muted/50 flex h-full w-full flex-col border-l'>
      {/* Layers Section */}
      <div className='flex flex-col'>
        <Button
          variant='ghost'
          onClick={() => setLayersExpanded(!layersExpanded)}
          className='hover:bg-accent/50 border-border flex h-auto w-full justify-start gap-1.5 rounded-none border-b px-2 py-1.5 text-left'
        >
          <motion.div
            animate={{ rotate: layersExpanded ? 0 : -90 }}
            transition={{ duration: 0.15, ease: 'easeOut' }}
          >
            <ChevronDownIcon className='text-muted-foreground size-3.5' />
          </motion.div>
          <LayersIcon className='size-3.5' />
          <span className='text-xs font-semibold tracking-wide uppercase'>
            {t('layers.title')}
          </span>
        </Button>
        <AnimatePresence initial={false}>
          {layersExpanded && (
            <motion.div
              initial={{ height: 0, opacity: 0 }}
              animate={{ height: 'auto', opacity: 1 }}
              exit={{ height: 0, opacity: 0 }}
              transition={{ duration: 0.2, ease: 'easeOut' }}
              className='border-border overflow-hidden border-b'
            >
              <LayersPanel />
            </motion.div>
          )}
        </AnimatePresence>
      </div>

      {/* Text Blocks Section - takes remaining space */}
      <TextBlocksPanel />
    </div>
  )
}
