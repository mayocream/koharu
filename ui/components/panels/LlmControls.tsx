'use client'

import { useEffect, useState } from 'react'
import { Select } from 'radix-ui'
import { useTranslation } from 'react-i18next'
import { useAppStore } from '@/lib/store'
import { TooltipButton } from '@/components/ui/form-controls'

export function LlmControls() {
  const {
    llmModels,
    llmSelectedModel,
    llmReady,
    llmLoading,
    llmList,
    llmSetSelectedModel,
    llmToggleLoadUnload,
    llmGenerate,
    llmCheckReady,
    render,
  } = useAppStore()
  const [generating, setGenerating] = useState(false)
  const { t } = useTranslation()

  useEffect(() => {
    llmList()
    llmCheckReady()
    const interval = setInterval(llmCheckReady, 1500)
    return () => clearInterval(interval)
  }, [llmList, llmCheckReady])

  return (
    <div className='space-y-2 text-xs text-neutral-600'>
      <div className='flex items-center gap-2 text-sm font-semibold text-neutral-900'>
        {t('llm.title')}{' '}
        <StatusBadge
          ready={llmReady}
          readyLabel={t('llm.ready')}
          idleLabel={t('llm.idle')}
        />
      </div>
      <Select.Root value={llmSelectedModel} onValueChange={llmSetSelectedModel}>
        <Select.Trigger className='inline-flex w-full items-center justify-between gap-2 rounded border border-neutral-200 bg-white px-2 py-1 text-sm hover:bg-neutral-50'>
          <Select.Value placeholder={t('llm.selectPlaceholder')} />
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
          label={!llmReady ? t('llm.load') : t('llm.unload')}
          tooltip={!llmReady ? t('llm.loadTooltip') : t('llm.unloadTooltip')}
          widthClass='w-full'
          onClick={llmToggleLoadUnload}
          disabled={!llmSelectedModel || llmLoading}
        />
        <TooltipButton
          label={generating ? t('llm.generating') : t('llm.generate')}
          tooltip={t('llm.generateTooltip')}
          disabled={!llmReady || generating}
          onClick={async () => {
            setGenerating(true)
            try {
              await llmGenerate(null)
            } finally {
              setGenerating(false)
            }
          }}
          widthClass='w-full'
        />
      </div>
      <div className='col flex'>
        <TooltipButton
          label={t('llm.render')}
          tooltip={t('llm.renderTooltip')}
          onClick={render}
          widthClass='w-full'
        />
      </div>
    </div>
  )
}

function StatusBadge({
  ready,
  readyLabel,
  idleLabel,
}: {
  ready: boolean
  readyLabel: string
  idleLabel: string
}) {
  return (
    <span className='inline-flex items-center gap-1 rounded border border-neutral-200 px-2 py-0.5 text-[11px]'>
      <span
        className={`h-2 w-2 rounded-full ${
          ready ? 'bg-rose-500' : 'bg-neutral-300'
        }`}
      />
      <span className={ready ? 'text-rose-600' : 'text-neutral-500'}>
        {ready ? readyLabel : idleLabel}
      </span>
    </span>
  )
}
