'use client'

import { Separator } from 'radix-ui'
import { useTranslation } from 'react-i18next'
import { useAppStore } from '@/lib/store'
import { ToggleField, TooltipButton } from '@/components/ui/form-controls'

export function MaskControls() {
  const {
    showSegmentationMask,
    setShowSegmentationMask,
    showInpaintedImage,
    setShowInpaintedImage,
    showRenderedImage,
    setShowRenderedImage,
    inpaint,
  } = useAppStore()
  const { t } = useTranslation()

  return (
    <div className='space-y-2 text-xs text-neutral-600'>
      <ToggleField
        label={t('mask.showSegmentationMask')}
        checked={showSegmentationMask}
        onChange={setShowSegmentationMask}
      />
      <ToggleField
        label={t('mask.showInpainted')}
        checked={showInpaintedImage}
        onChange={setShowInpaintedImage}
      />
      <ToggleField
        label={t('mask.showRendered')}
        checked={showRenderedImage}
        onChange={setShowRenderedImage}
      />
      <Separator.Root className='my-1 h-px bg-neutral-200' />
      <div className='flex'>
        <TooltipButton
          label={t('mask.inpaint')}
          tooltip={t('mask.inpaintTooltip')}
          widthClass='w-full'
          onClick={inpaint}
        />
      </div>
    </div>
  )
}
