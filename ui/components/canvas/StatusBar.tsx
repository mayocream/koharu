'use client'

import { Slider } from 'radix-ui'
import { useTranslation } from 'react-i18next'
import { useCanvasZoom } from '@/hooks/useCanvasZoom'

export function StatusBar() {
  const { scale, setScale, summary } = useCanvasZoom()
  const { t } = useTranslation()

  return (
    <div className='flex items-center justify-end gap-3 border-t border-neutral-300 px-2 py-1 text-xs'>
      <div className='flex items-center gap-1.5'>
        <span className='text-neutral-500'>{t('statusBar.zoom')}</span>
        <div className='w-44'>
          <Slider.Root
            className='relative flex h-4 w-full touch-none items-center select-none'
            min={10}
            max={100}
            step={5}
            value={[scale]}
            onValueChange={(v) => setScale(v[0] ?? scale)}
          >
            <Slider.Track className='relative h-1 flex-1 rounded bg-rose-100'>
              <Slider.Range className='absolute h-full rounded bg-rose-400' />
            </Slider.Track>
            <Slider.Thumb className='block h-2.5 w-2.5 rounded-full bg-rose-500' />
          </Slider.Root>
        </div>
        <span className='w-10 text-right tabular-nums'>{scale}%</span>
      </div>
      <span className='ml-auto text-[11px] text-neutral-600'>
        {t('statusBar.canvas')}: {summary}
      </span>
    </div>
  )
}
