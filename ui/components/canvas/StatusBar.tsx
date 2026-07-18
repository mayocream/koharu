'use client'

import { useTranslation } from 'react-i18next'

import { Slider } from '@/components/ui/slider'
import { koharuClient, useEditorStore } from '@/lib/koharu'

export function StatusBar() {
  const { t } = useTranslation()
  const revision = useEditorStore((state) => state.revision)
  const page = useEditorStore((state) => state.page)
  const pages = useEditorStore((state) => state.pages)
  const camera = useEditorStore((state) => state.camera)
  const pageIndex = page ? pages.findIndex((item) => item.id === page.id) + 1 : 0
  const percent = Math.round(camera.zoom * 100)
  const sliderPercent = Math.min(800, Math.max(10, percent))

  return (
    <footer className='flex shrink-0 items-center justify-end gap-3 border-t border-border bg-card px-2 py-1 text-xs text-foreground'>
      <div className='flex items-center gap-1.5'>
        <span className='text-muted-foreground'>
          {t('native.status.zoom', { defaultValue: 'Zoom' })}
        </span>
        <Slider
          aria-label={t('native.status.zoom', { defaultValue: 'Zoom' })}
          className='w-44 [&_[data-slot=slider-range]]:bg-primary [&_[data-slot=slider-thumb]]:size-2.5 [&_[data-slot=slider-thumb]]:border-primary [&_[data-slot=slider-thumb]]:bg-primary [&_[data-slot=slider-track]]:bg-primary/20'
          min={10}
          max={800}
          step={5}
          value={[sliderPercent]}
          onValueChange={(value) =>
            koharuClient.interact({
              type: 'set_zoom',
              zoom: (value[0] ?? percent) / 100,
            })
          }
        />
        <button
          className='w-10 text-right tabular-nums'
          aria-label={t('native.canvas.fit', { defaultValue: 'Fit Window' })}
          title={t('native.canvas.fit', { defaultValue: 'Fit Window' })}
          onClick={() => koharuClient.interact({ type: 'fit_window' })}
        >
          {percent}%
        </button>
      </div>
      <span className='ml-auto text-[11px] text-muted-foreground'>
        {t('native.status.revision', { defaultValue: 'Revision' })} {revision}
        {page ? ` · ${pageIndex}/${pages.length} · ${page.name}` : ''}
        {camera.autoFit ? ` · ${t('native.canvas.autoFit', { defaultValue: 'Auto fit' })}` : ''}
      </span>
    </footer>
  )
}
