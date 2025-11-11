'use client'

import type { ComponentType } from 'react'
import { Toolbar } from 'radix-ui'
import { MousePointer, Square, Brush } from 'lucide-react'
import { useAppStore } from '@/lib/store'
import { ToolMode } from '@/types'

type ModeDefinition = {
  label: string
  value: ToolMode
  icon: ComponentType<{ className?: string }>
}

const MODES: ModeDefinition[] = [
  { label: 'Select', value: 'select', icon: MousePointer },
  { label: 'Block', value: 'block', icon: Square },
  { label: 'Mask', value: 'mask', icon: Brush },
]

export function ToolRail() {
  const mode = useAppStore((state) => state.mode)
  const setMode = useAppStore((state) => state.setMode)

  return (
    <div className='flex w-12 flex-col border-r border-neutral-200 bg-white'>
      <Toolbar.Root
        orientation='vertical'
        className='flex flex-1 flex-col items-center gap-1.5 py-3'
      >
        {MODES.map((item) => (
          <Toolbar.Button
            key={item.value}
            data-active={item.value === mode}
            onClick={() => setMode(item.value)}
            className='flex h-8 w-8 items-center justify-center rounded border border-transparent text-neutral-600 hover:border-neutral-300 data-[active=true]:border-rose-400 data-[active=true]:bg-rose-50 data-[active=true]:text-rose-600'
            aria-label={item.label}
          >
            <item.icon className='h-4 w-4' />
          </Toolbar.Button>
        ))}
      </Toolbar.Root>
    </div>
  )
}
