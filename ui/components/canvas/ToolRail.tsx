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
import { useAppStore } from '@/lib/store'
import { ToolMode } from '@/types'
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip'

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
  const mode = useAppStore((state) => state.mode)
  const setMode = useAppStore((state) => state.setMode)
  const { t } = useTranslation()

  return (
    <div className='flex w-12 flex-col border-r border-neutral-200 bg-white'>
      <div className='flex flex-1 flex-col items-center gap-1.5 py-3'>
        {MODES.map((item) => {
          const label = t(item.labelKey)
          return (
            <Tooltip key={item.value} delayDuration={1000}>
              <TooltipTrigger asChild>
                <button
                  data-active={item.value === mode}
                  onClick={() => setMode(item.value)}
                  className='flex h-8 w-8 items-center justify-center rounded border border-transparent text-neutral-600 hover:border-neutral-300 data-[active=true]:border-rose-400 data-[active=true]:bg-rose-50 data-[active=true]:text-rose-600'
                  aria-label={label}
                >
                  <item.icon className='h-4 w-4' />
                </button>
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
