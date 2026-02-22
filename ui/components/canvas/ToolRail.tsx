'use client'

import type { ComponentType } from 'react'
import { useTranslation } from 'react-i18next'
import {
  MousePointer,
  VectorSquare,
  Brush,
  Bandage,
  Eraser,
} from 'lucide-react'
import { useConfigStore } from '@/lib/store'
import type { ToolMode } from '@/types'
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '@/components/ui/popover'
import { Slider } from '@/components/ui/slider'
import { Button } from '@/components/ui/button'
import { ColorPicker } from '@/components/ui/color-picker'
import {
  selectBrushConfig,
  selectSetBrushConfig,
  useAppShallow,
} from '@/lib/store-selectors'

type ModeDefinition = {
  value: ToolMode
  icon: ComponentType<{ className?: string }>
  labelKey: string
}

const MODES: ModeDefinition[] = [
  { labelKey: 'toolRail.select', value: 'select', icon: MousePointer },
  { labelKey: 'toolRail.block', value: 'block', icon: VectorSquare },
  { labelKey: 'toolRail.brush', value: 'brush', icon: Brush },
  { labelKey: 'toolRail.eraser', value: 'eraser', icon: Eraser },
  { labelKey: 'toolRail.repairBrush', value: 'repairBrush', icon: Bandage },
]

export function ToolRail() {
  const { mode, setMode } = useAppShallow((state) => ({
    mode: state.mode,
    setMode: state.setMode,
  }))
  const { t } = useTranslation()

  return (
    <div className='border-border bg-card flex w-11 flex-col border-r'>
      <div className='flex flex-1 flex-col items-center gap-1 py-2'>
        {MODES.map((item) => {
          const label = t(item.labelKey)

          // Brush tool gets a popover
          if (item.value === 'brush') {
            return (
              <BrushToolWithPopover
                key={item.value}
                item={item}
                label={label}
                isActive={item.value === mode}
                onSelect={() => setMode(item.value)}
              />
            )
          }

          return (
            <Tooltip key={item.value}>
              <TooltipTrigger asChild>
                <Button
                  variant='ghost'
                  size='icon-sm'
                  data-active={item.value === mode}
                  onClick={() => setMode(item.value)}
                  className='text-muted-foreground data-[active=true]:border-primary data-[active=true]:bg-accent data-[active=true]:text-primary border border-transparent'
                  aria-label={label}
                >
                  <item.icon className='h-4 w-4' />
                </Button>
              </TooltipTrigger>
              <TooltipContent side='right' sideOffset={8}>
                {label}
              </TooltipContent>
            </Tooltip>
          )
        })}
      </div>
    </div>
  )
}

function BrushToolWithPopover({
  item,
  label,
  isActive,
  onSelect,
}: {
  item: ModeDefinition
  label: string
  isActive: boolean
  onSelect: () => void
}) {
  const { size: brushSize, color: brushColor } =
    useConfigStore(selectBrushConfig)
  const setBrushConfig = useConfigStore(selectSetBrushConfig)
  const { t } = useTranslation()

  return (
    <Popover>
      <Tooltip>
        <TooltipTrigger asChild>
          <PopoverTrigger asChild>
            <Button
              variant='ghost'
              size='icon-sm'
              data-active={isActive}
              onClick={onSelect}
              className='text-muted-foreground data-[active=true]:border-primary data-[active=true]:bg-accent data-[active=true]:text-primary border border-transparent'
              aria-label={label}
            >
              <item.icon className='h-4 w-4' />
            </Button>
          </PopoverTrigger>
        </TooltipTrigger>
        <TooltipContent side='right' sideOffset={8}>
          {label}
        </TooltipContent>
      </Tooltip>
      <PopoverContent side='right' align='start' className='w-56'>
        <div className='space-y-4 text-sm'>
          <div className='space-y-2'>
            <p className='text-muted-foreground text-xs font-medium uppercase'>
              {t('toolbar.brushSize')}
            </p>
            <div className='flex items-center gap-2'>
              <Slider
                className='[&_[data-slot=slider-range]]:bg-primary [&_[data-slot=slider-thumb]]:border-primary [&_[data-slot=slider-thumb]]:bg-primary [&_[data-slot=slider-track]]:bg-primary/20 flex-1 [&_[data-slot=slider-thumb]]:size-3'
                min={8}
                max={128}
                step={4}
                value={[brushSize]}
                onValueChange={(vals) =>
                  setBrushConfig({ size: vals[0] ?? brushSize })
                }
              />
              <span className='text-muted-foreground w-10 text-right tabular-nums'>
                {brushSize}px
              </span>
            </div>
          </div>
          <div className='space-y-2'>
            <p className='text-muted-foreground text-xs font-medium uppercase'>
              {t('toolbar.brushColor')}
            </p>
            <div className='flex items-center gap-2'>
              <ColorPicker
                value={brushColor}
                onChange={(color) => setBrushConfig({ color })}
                className='h-8 w-8'
              />
              <span className='text-muted-foreground text-xs'>
                {brushColor}
              </span>
            </div>
          </div>
        </div>
      </PopoverContent>
    </Popover>
  )
}
