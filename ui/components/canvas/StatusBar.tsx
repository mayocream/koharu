'use client'

import { useTranslation } from 'react-i18next'

import { Slider } from '@/components/ui/slider'
import { useCanvasZoom } from '@/hooks/useCanvasZoom'

export function StatusBar() {
  const { scale, setScale, summary } = useCanvasZoom()
  const { t } = useTranslation()

  return (
    <div className='flex shrink-0 items-center justify-end gap-3 border-t border-border bg-card px-2 py-1 text-xs text-foreground'>
      <div className='flex items-center gap-1.5'>
        <span className='text-muted-foreground'>{t('statusBar.zoom')}</span>
        <Slider
          data-testid='zoom-slider'
          className='w-44 [&_[data-slot=slider-range]]:bg-primary [&_[data-slot=slider-thumb]]:size-2.5 [&_[data-slot=slider-thumb]]:border-primary [&_[data-slot=slider-thumb]]:bg-primary [&_[data-slot=slider-track]]:bg-primary/20'
          min={10}
          max={100}
          step={5}
          value={[scale]}
          onValueChange={(v) => setScale(v[0] ?? scale)}
        />
        <span data-testid='zoom-value' className='w-10 text-right tabular-nums'>
          {scale}%
        </span>
      </div>
      <span className='ml-auto text-[11px] text-muted-foreground'>
        {t('statusBar.canvas')}: {summary}
      </span>
    </div>
  )
}
