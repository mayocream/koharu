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
    <div className='border-border bg-card text-foreground flex items-center gap-4 border-b px-3 py-1.5 text-xs'>
      <div className='flex items-center gap-3'>
        <span className='text-muted-foreground'>{t('toolbar.brushSize')}</span>
        <Slider
          className='[&_[data-slot=slider-range]]:bg-primary [&_[data-slot=slider-thumb]]:border-primary [&_[data-slot=slider-thumb]]:bg-primary [&_[data-slot=slider-track]]:bg-primary/20 w-40 [&_[data-slot=slider-thumb]]:size-2.5'
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
            <span className='text-muted-foreground'>
              {t('toolbar.brushColor')}
            </span>
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
          llmReady
            ? 'bg-primary/20 text-primary'
            : 'bg-muted text-muted-foreground'
        }`}
      >
        {llmReady ? t('llm.statusReady') : t('llm.statusIdle')}
      </span>
    </div>
  )
}
