'use client'

import { useTranslation } from 'react-i18next'
import { useAppStore, useConfigStore } from '@/lib/store'
import { Slider } from '@/components/ui/slider'

export function CanvasToolbar() {
  const llmReady = useAppStore((state) => state.llmReady)
  const mode = useAppStore((state) => state.mode)
  const {
    brushConfig: { size: brushSize, color: brushColor },
    setBrushConfig,
  } = useConfigStore()
  const { t } = useTranslation()

  return (
    <div className='flex items-center gap-4 border-b border-neutral-200 bg-white px-3 py-1.5 text-xs text-neutral-900'>
      <div className='flex items-center gap-3'>
        <span className='text-neutral-600'>{t('toolbar.brushSize')}</span>
        <Slider
          className='w-40 [&_[data-slot=slider-range]]:bg-rose-400 [&_[data-slot=slider-thumb]]:size-2.5 [&_[data-slot=slider-thumb]]:border-rose-500 [&_[data-slot=slider-thumb]]:bg-rose-500 [&_[data-slot=slider-track]]:bg-rose-100'
          min={8}
          max={128}
          step={4}
          value={[brushSize]}
          onValueChange={(vals) =>
            setBrushConfig({ size: vals[0] ?? brushSize })
          }
        />
        <span className='w-14 text-right tabular-nums'>{brushSize}px</span>
        {mode === 'brush' && (
          <label className='flex items-center gap-2'>
            <span className='text-neutral-600'>{t('toolbar.brushColor')}</span>
            <input
              type='color'
              value={brushColor}
              onChange={(event) =>
                setBrushConfig({ color: event.target.value })
              }
              className='h-6 w-6 cursor-pointer appearance-none border-none p-0'
              aria-label={t('toolbar.brushColor')}
            />
          </label>
        )}
      </div>
      <span
        className={`ml-auto rounded-sm px-2 py-1 text-xs ${
          llmReady ? 'bg-rose-100 text-rose-700' : 'bg-rose-50 text-rose-400'
        }`}
      >
        {llmReady ? t('llm.statusReady') : t('llm.statusIdle')}
      </span>
    </div>
  )
}
