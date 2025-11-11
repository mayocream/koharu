'use client'

import { useState } from 'react'
import { Separator } from 'radix-ui'
import { useAppStore, useConfigStore } from '@/lib/store'
import {
  SliderField,
  ToggleField,
  TooltipButton,
} from '@/components/ui/form-controls'

export function MaskControls() {
  const {
    showSegmentationMask,
    setShowSegmentationMask,
    showInpaintedImage,
    setShowInpaintedImage,
    inpaint,
  } = useAppStore()
  const { inpaintConfig, setInpaintConfig } = useConfigStore()
  const [brushSize, setBrushSize] = useState(36)

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
      <Separator.Root className='my-1 h-px bg-neutral-200' />
      <SliderField
        label='Brush size'
        min={8}
        max={128}
        step={4}
        value={brushSize}
        onChange={setBrushSize}
      />
      <div className='grid grid-cols-2 gap-1.5'>
        <SliderField
          label='Dilate'
          min={1}
          max={20}
          step={1}
          value={inpaintConfig.dilateKernelSize}
          onChange={(value) => setInpaintConfig({ dilateKernelSize: value })}
          formatValue={(value) => value.toString()}
        />
        <SliderField
          label='Erode'
          min={1}
          max={10}
          step={1}
          value={inpaintConfig.erodeDistance}
          onChange={(value) => setInpaintConfig({ erodeDistance: value })}
          formatValue={(value) => value.toString()}
        />
      </div>
      <TooltipButton
        label='Inpaint'
        tooltip='Apply inpainting'
        widthClass='w-full'
        onClick={() =>
          inpaint(inpaintConfig.dilateKernelSize, inpaintConfig.erodeDistance)
        }
      />
    </div>
  )
}
