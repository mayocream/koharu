'use client'

import { useAppStore } from '@/lib/store'
import { useTranslation } from 'react-i18next'
import { ScanSearchIcon, TextIcon, EraserIcon } from 'lucide-react'
import { Button } from '@/components/ui/button'
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip'

export function ProcessingControls() {
  const { inpaint, detect, ocr } = useAppStore()
  const { t } = useTranslation()

  return (
    <div className='flex gap-1'>
      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            variant='outline'
            size='sm'
            onClick={detect}
            className='flex-1 gap-1.5 text-xs'
          >
            <ScanSearchIcon className='size-3.5' />
            {t('processing.detect')}
          </Button>
        </TooltipTrigger>
        <TooltipContent side='bottom' sideOffset={4}>
          {t('processing.detectTooltip')}
        </TooltipContent>
      </Tooltip>
      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            variant='outline'
            size='sm'
            onClick={ocr}
            className='flex-1 gap-1.5 text-xs'
          >
            <TextIcon className='size-3.5' />
            {t('processing.ocr')}
          </Button>
        </TooltipTrigger>
        <TooltipContent side='bottom' sideOffset={4}>
          {t('processing.ocrTooltip')}
        </TooltipContent>
      </Tooltip>
      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            variant='outline'
            size='sm'
            onClick={inpaint}
            className='flex-1 gap-1.5 text-xs'
          >
            <EraserIcon className='size-3.5' />
            {t('mask.inpaint')}
          </Button>
        </TooltipTrigger>
        <TooltipContent side='bottom' sideOffset={4}>
          {t('mask.inpaintTooltip')}
        </TooltipContent>
      </Tooltip>
    </div>
  )
}
