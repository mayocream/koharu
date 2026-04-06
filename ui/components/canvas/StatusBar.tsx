'use client'

import { useTranslation } from 'react-i18next'
import { useCanvasZoom } from '@/hooks/useCanvasZoom'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { Slider } from '@/components/ui/slider'
import { Progress } from '@/components/ui/progress'

const STAGE_LABELS: Record<'detect' | 'ocr' | 'translate', string> = {
  detect: 'statusBar.detecting',
  ocr: 'statusBar.ocr',
  translate: 'statusBar.translating',
}

export function StatusBar() {
  const { scale, setScale, summary } = useCanvasZoom()
  const { t } = useTranslation()
  const prefetchProgress = useEditorUiStore((state) => state.prefetchProgress)
  const processingProgress = useEditorUiStore((state) => state.processingProgress)

  return (
    <div className='border-border bg-card text-foreground flex shrink-0 items-center justify-end gap-3 border-t px-2 py-1 text-xs'>
      {processingProgress && (
        <div className='flex items-center gap-2'>
          <span className='text-muted-foreground'>{t(STAGE_LABELS[processingProgress.stage])}</span>
          <Progress
            value={(processingProgress.current / processingProgress.total) * 100}
            className='h-1.5 w-24'
          />
          <span className='text-muted-foreground tabular-nums'>
            {processingProgress.current}/{processingProgress.total}
          </span>
        </div>
      )}
      {prefetchProgress && !processingProgress && (
        <div className='flex items-center gap-2'>
          <span className='text-muted-foreground'>{t('statusBar.caching')}</span>
          <Progress
            value={(prefetchProgress.loaded / prefetchProgress.total) * 100}
            className='h-1.5 w-24'
          />
          <span className='text-muted-foreground tabular-nums'>
            {prefetchProgress.loaded}/{prefetchProgress.total}
          </span>
        </div>
      )}
      <div className='flex items-center gap-1.5'>
        <span className='text-muted-foreground'>{t('statusBar.zoom')}</span>
        <Slider
          data-testid='zoom-slider'
          className='[&_[data-slot=slider-range]]:bg-primary [&_[data-slot=slider-thumb]]:border-primary [&_[data-slot=slider-thumb]]:bg-primary [&_[data-slot=slider-track]]:bg-primary/20 w-44 [&_[data-slot=slider-thumb]]:size-2.5'
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
      <span className='text-muted-foreground ml-auto text-[11px]'>
        {t('statusBar.canvas')}: {summary}
      </span>
    </div>
  )
}
