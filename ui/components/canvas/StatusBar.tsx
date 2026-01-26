'use client'

import { useTranslation } from 'react-i18next'
import { useCanvasZoom } from '@/hooks/useCanvasZoom'
import { Slider } from '@/components/ui/slider'

export function StatusBar() {
  const { scale, setScale, summary } = useCanvasZoom()
  const { t } = useTranslation()

  return (
    <div className='flex items-center justify-end gap-3 border-t border-neutral-300 px-2 py-1 text-xs'>
      <div className='flex items-center gap-1.5'>
        <span className='text-neutral-500'>{t('statusBar.zoom')}</span>
        <Slider
          className='w-44 [&_[data-slot=slider-range]]:bg-rose-400 [&_[data-slot=slider-thumb]]:size-2.5 [&_[data-slot=slider-thumb]]:border-rose-500 [&_[data-slot=slider-thumb]]:bg-rose-500 [&_[data-slot=slider-track]]:bg-rose-100'
          min={10}
          max={100}
          step={5}
          value={[scale]}
          onValueChange={(v) => setScale(v[0] ?? scale)}
        />
        <span className='w-10 text-right tabular-nums'>{scale}%</span>
      </div>
      <span className='ml-auto text-[11px] text-neutral-600'>
        {t('statusBar.canvas')}: {summary}
      </span>
    </div>
  )
}
