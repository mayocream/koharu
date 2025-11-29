'use client'

import { Separator } from 'radix-ui'
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

  return (
    <div className='space-y-2 text-xs text-neutral-600'>
      <ToggleField
        label='Show segmentation mask'
        checked={showSegmentationMask}
        onChange={setShowSegmentationMask}
      />
      <ToggleField
        label='Show inpainted image'
        checked={showInpaintedImage}
        onChange={setShowInpaintedImage}
      />
      <ToggleField
        label='Show rendered text'
        checked={showRenderedImage}
        onChange={setShowRenderedImage}
      />
      <Separator.Root className='my-1 h-px bg-neutral-200' />
      <TooltipButton
        label='Inpaint'
        tooltip='Apply inpainting'
        widthClass='w-full'
        onClick={inpaint}
      />
    </div>
  )
}
