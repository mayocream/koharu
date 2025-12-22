'use client'

import { useAppStore } from '@/lib/store'
import { ToggleField, TooltipButton } from '@/components/ui/form-controls'
import { useTranslation } from 'react-i18next'
import { Separator } from 'radix-ui'

export function ProcessingControls() {
  const {
    showSegmentationMask,
    setShowSegmentationMask,
    showInpaintedImage,
    setShowInpaintedImage,
    showBrushLayer,
    setShowBrushLayer,
    showTextBlocksOverlay,
    setShowTextBlocksOverlay,
    documents,
    currentDocumentIndex,
    inpaint,
    detect,
    ocr,
  } = useAppStore()
  const { t } = useTranslation()
  const currentDocument = documents[currentDocumentIndex]

  return (
    <div className='space-y-2 text-xs text-neutral-600'>
      <Separator.Root className='my-1 h-px bg-neutral-200' />
      <ToggleField
        label={t('mask.showInpainted')}
        checked={showInpaintedImage}
        onChange={setShowInpaintedImage}
        disabled={currentDocument?.inpainted === undefined}
      />
      <ToggleField
        label={t('mask.showSegmentationMask')}
        checked={showSegmentationMask}
        onChange={setShowSegmentationMask}
        disabled={currentDocument?.segment === undefined}
      />
      <ToggleField
        label={t('mask.showBrushLayer')}
        checked={showBrushLayer}
        onChange={setShowBrushLayer}
        disabled={currentDocument?.inpainted === undefined}
      />
      <ToggleField
        label={t('mask.showTextBlocks')}
        checked={showTextBlocksOverlay}
        onChange={setShowTextBlocksOverlay}
        disabled={currentDocument?.textBlocks === undefined}
      />
      <Separator.Root className='my-1 h-px bg-neutral-200' />
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
