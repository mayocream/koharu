'use client'

import { Tabs } from 'radix-ui'
import { ProcessingControls } from '@/components/panels/ProcessingControls'
import { MaskControls } from '@/components/panels/MaskControls'
import { LlmControls } from '@/components/panels/LlmControls'
import { TextBlocksPanel } from '@/components/panels/TextBlocksPanel'

const PANEL_TABS = [
  { value: 'processing', label: 'Processing', Component: ProcessingControls },
  { value: 'mask', label: 'Mask', Component: MaskControls },
  { value: 'llm', label: 'LLM', Component: LlmControls },
] as const

export function Panels() {
  return (
    <div className='flex w-64 shrink-0 flex-col border-l border-neutral-200 bg-neutral-50'>
      <Tabs.Root
        defaultValue='processing'
        className='border-b border-neutral-200'
      >
        <Tabs.List className='grid grid-cols-3 bg-white text-[11px] font-semibold tracking-wide text-neutral-600 uppercase'>
          {PANEL_TABS.map((tab) => (
            <Tabs.Trigger
              key={tab.value}
              value={tab.value}
              className='px-2.5 py-1.5 hover:bg-neutral-100'
            >
              {tab.label}
            </Tabs.Trigger>
          ))}
        </Tabs.List>
        <div className='px-2.5 py-2'>
          {PANEL_TABS.map(({ value, Component }) => (
            <Tabs.Content key={value} value={value}>
              <Component />
            </Tabs.Content>
          ))}
        </div>
      </Tabs.Root>
      <TextBlocksPanel />
    </div>
  )
}
