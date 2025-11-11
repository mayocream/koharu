'use client'

import { useEffect, useState } from 'react'
import { Select } from 'radix-ui'
import { useAppStore } from '@/lib/store'
import { TextareaField, TooltipButton } from '@/components/ui/form-controls'

export function LlmControls() {
  const {
    llmModels,
    llmSelectedModel,
    llmReady,
    llmSystemPrompt,
    llmList,
    llmSetSelectedModel,
    llmLoad,
    llmOffload,
    llmSetSystemPrompt,
    llmGenerate,
    llmCheckReady,
  } = useAppStore()
  const [generating, setGenerating] = useState(false)

  useEffect(() => {
    llmList()
    llmCheckReady()
    const interval = setInterval(llmCheckReady, 1500)
    return () => clearInterval(interval)
  }, [llmList, llmCheckReady])

  return (
    <div className='space-y-2 text-xs text-neutral-600'>
      <div className='flex items-center gap-2 text-sm font-semibold text-neutral-900'>
        LLM <StatusBadge ready={llmReady} />
      </div>
      <Select.Root value={llmSelectedModel} onValueChange={llmSetSelectedModel}>
        <Select.Trigger className='inline-flex w-full items-center justify-between gap-2 rounded border border-neutral-200 bg-white px-2 py-1 text-sm hover:bg-neutral-50'>
          <Select.Value placeholder='Select model' />
        </Select.Trigger>
        <Select.Portal>
          <Select.Content className='min-w-56 rounded-md bg-white p-1 shadow-sm'>
            <Select.Viewport>
              {llmModels.map((model) => (
                <Select.Item
                  key={model}
                  value={model}
                  className='rounded px-3 py-1.5 text-sm outline-none select-none hover:bg-black/5 data-[state=checked]:bg-black/5'
                >
                  <Select.ItemText>{model}</Select.ItemText>
                </Select.Item>
              ))}
            </Select.Viewport>
          </Select.Content>
        </Select.Portal>
      </Select.Root>
      <div className='flex gap-2'>
        <TooltipButton
          label='Load'
          tooltip='Load selected model'
          widthClass='w-full'
          onClick={llmLoad}
          disabled={!llmSelectedModel}
        />
        <TooltipButton
          label='Offload'
          tooltip='Release model from memory'
          widthClass='w-full'
          onClick={llmOffload}
        />
      </div>
      <TextareaField
        label='System prompt'
        value={llmSystemPrompt}
        placeholder='Describe how the assistant should rewrite text'
        onChange={llmSetSystemPrompt}
      />
      <div className='flex justify-end'>
        <button
          type='button'
          onClick={async () => {
            setGenerating(true)
            try {
              await llmGenerate()
            } finally {
              setGenerating(false)
            }
          }}
          disabled={!llmReady || generating}
          className='rounded border border-neutral-200 bg-white px-3 py-1.5 text-sm font-semibold hover:bg-neutral-100 disabled:opacity-50'
        >
          {generating ? 'Generating...' : 'Generate'}
        </button>
      </div>
    </div>
  )
}

function StatusBadge({ ready }: { ready: boolean }) {
  return (
    <span className='inline-flex items-center gap-1 rounded border border-neutral-200 px-2 py-0.5 text-[11px]'>
      <span
        className={`h-2 w-2 rounded-full ${
          ready ? 'bg-rose-500' : 'bg-neutral-300'
        }`}
      />
      <span className={ready ? 'text-rose-600' : 'text-neutral-500'}>
        {ready ? 'Ready' : 'Idle'}
      </span>
    </span>
  )
}
