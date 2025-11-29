'use client'

import { useAppStore } from '@/lib/store'
import { TooltipButton } from '@/components/ui/form-controls'

export function ProcessingControls() {
  const { detect, ocr } = useAppStore()

  return (
    <div className='space-y-2 text-xs text-neutral-600'>
      <div className='flex gap-2'>
        <TooltipButton
          label='Detect'
          tooltip='Run text detection on current page'
          onClick={detect}
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
