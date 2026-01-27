'use client'

import { useTranslation } from 'react-i18next'
import { useCanvasZoom } from '@/hooks/useCanvasZoom'
import { Slider } from '@/components/ui/slider'

export function StatusBar() {
  const { scale, setScale, summary } = useCanvasZoom()
  const { t } = useTranslation()

  return (
    <div className='border-border bg-card text-foreground flex items-center justify-end gap-3 border-t px-2 py-1 text-xs'>
      <div className='flex items-center gap-1.5'>
        <span className='text-muted-foreground'>{t('statusBar.zoom')}</span>
        <Slider
          className='[&_[data-slot=slider-range]]:bg-primary [&_[data-slot=slider-thumb]]:border-primary [&_[data-slot=slider-thumb]]:bg-primary [&_[data-slot=slider-track]]:bg-primary/20 w-44 [&_[data-slot=slider-thumb]]:size-2.5'
          min={10}
          max={100}
          step={5}
          value={[scale]}
          onValueChange={(v) => setScale(v[0] ?? scale)}
        />
        <span className='w-10 text-right tabular-nums'>{scale}%</span>
      </div>
      <span className='text-muted-foreground ml-auto text-[11px]'>
        {t('statusBar.canvas')}: {summary}
      </span>
    </div>
  )
}
