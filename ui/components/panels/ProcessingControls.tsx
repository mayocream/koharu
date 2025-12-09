'use client'

import { useAppStore } from '@/lib/store'
import { TooltipButton } from '@/components/ui/form-controls'
import { useTranslation } from 'react-i18next'

export function ProcessingControls() {
  const { detect, ocr } = useAppStore()
  const { t } = useTranslation()

  return (
    <div className='space-y-2 text-xs text-neutral-600'>
      <div className='flex gap-2'>
        <TooltipButton
          label={t('processing.detect')}
          tooltip={t('processing.detectTooltip')}
          onClick={detect}
          widthClass='w-full'
        />
        <TooltipButton
          label={t('processing.ocr')}
          tooltip={t('processing.ocrTooltip')}
          onClick={ocr}
          widthClass='w-full'
        />
      </div>
    </div>
  )
}
