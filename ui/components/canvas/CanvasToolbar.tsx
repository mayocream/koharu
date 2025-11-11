'use client'

import { Toolbar, Tooltip } from 'radix-ui'
import { useCanvasCommands } from '@/hooks/useCanvasCommands'

export function CanvasToolbar() {
  const { commands, llmReady } = useCanvasCommands()

  return (
    <Toolbar.Root className='flex items-center gap-1 border-b border-neutral-200 bg-white px-2 py-1.5 text-xs text-neutral-900'>
      {commands.map((item) => (
        <Tooltip.Root key={item.label} delayDuration={0}>
          <Tooltip.Trigger asChild>
            <Toolbar.Button
              onClick={item.action}
              disabled={item.disabled}
              className='rounded border border-neutral-200 bg-white px-2.5 py-1 font-semibold hover:bg-neutral-100 disabled:opacity-40 data-[state=on]:bg-neutral-900 data-[state=on]:text-white'
            >
              {item.label}
            </Toolbar.Button>
          </Tooltip.Trigger>
          <Tooltip.Content
            sideOffset={6}
            className='rounded bg-black px-2 py-1 text-xs text-white'
          >
            Run {item.label.toLowerCase()}
          </Tooltip.Content>
        </Tooltip.Root>
      ))}
      <span
        className={`ml-auto rounded-full px-2 py-1 text-xs ${
          llmReady ? 'bg-rose-100 text-rose-700' : 'bg-rose-50 text-rose-400'
        }`}
      >
        {llmReady ? 'LLM Ready' : 'LLM Idle'}
      </span>
    </Toolbar.Root>
  )
}
