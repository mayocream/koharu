'use client'

import { Toolbar, Slider } from 'radix-ui'
import { useTranslation } from 'react-i18next'
import { useAppStore, useConfigStore } from '@/lib/store'

export function CanvasToolbar() {
  const llmReady = useAppStore((state) => state.llmReady)
  const {
    maskConfig: { brushSize },
    setMaskConfig,
  } = useConfigStore()
  const { t } = useTranslation()

  return (
    <Toolbar.Root className='flex items-center gap-4 border-b border-neutral-200 bg-white px-3 py-1.5 text-xs text-neutral-900'>
      <div className='flex items-center gap-2'>
        <span className='text-neutral-600'>{t('toolbar.brush')}</span>
        <div className='w-40'>
          <Slider.Root
            className='relative flex h-4 w-full touch-none items-center select-none'
            min={8}
            max={128}
            step={4}
            value={[brushSize]}
            onValueChange={(vals) =>
              setMaskConfig({ brushSize: vals[0] ?? brushSize })
            }
          >
            <Slider.Track className='relative h-1 flex-1 rounded bg-rose-100'>
              <Slider.Range className='absolute h-full rounded bg-rose-400' />
            </Slider.Track>
            <Slider.Thumb className='block h-2.5 w-2.5 rounded-full bg-rose-500' />
          </Slider.Root>
        </div>
        <span className='w-14 text-right tabular-nums'>{brushSize}px</span>
      </div>
      <span
        className={`ml-auto rounded-sm px-2 py-1 text-xs ${
          llmReady ? 'bg-rose-100 text-rose-700' : 'bg-rose-50 text-rose-400'
        }`}
      >
        {llmReady ? t('llm.statusReady') : t('llm.statusIdle')}
      </span>
    </Toolbar.Root>
  )
}
