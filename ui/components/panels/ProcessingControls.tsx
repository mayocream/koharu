'use client'

import { useAppStore, useConfigStore } from '@/lib/store'
import {
  SliderField,
  TooltipButton,
} from '@/components/ui/form-controls'

export function ProcessingControls() {
  const { detect, ocr } = useAppStore()
  const { detectConfig, setDetectConfig } = useConfigStore()

  return (
    <div className='space-y-2 text-xs text-neutral-600'>
      <SliderField
        label='Confidence threshold'
        min={0.1}
        max={1}
        step={0.05}
        value={detectConfig.confThreshold}
        onChange={(value) => setDetectConfig({ confThreshold: value })}
      />
      <SliderField
        label='NMS threshold'
        min={0.1}
        max={1}
        step={0.05}
        value={detectConfig.nmsThreshold}
        onChange={(value) => setDetectConfig({ nmsThreshold: value })}
      />
      <div className='flex gap-2'>
        <TooltipButton
          label='Detect'
          tooltip='Run text detection on current page'
          onClick={() =>
            detect(detectConfig.confThreshold, detectConfig.nmsThreshold)
          }
          widthClass='w-full'
        />
        <TooltipButton
          label='OCR'
          tooltip='Recognize text for detected regions'
          onClick={ocr}
          widthClass='w-full'
        />
      </div>
    </div>
  )
}
