'use client'

import { useAppStore } from '@/lib/store'
import { useTranslation } from 'react-i18next'
import { Separator } from '@/components/ui/separator'
import { Switch } from '@/components/ui/switch'
import { Button } from '@/components/ui/button'
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip'

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
    currentDocument,
    inpaint,
    detect,
    ocr,
  } = useAppStore()
  const { t } = useTranslation()

  return (
    <div className='space-y-2 text-xs text-neutral-600'>
      <Separator className='my-1' />
      <label className='flex items-center gap-2 text-sm'>
        <Switch
          size='sm'
          checked={showInpaintedImage}
          onCheckedChange={setShowInpaintedImage}
          disabled={currentDocument?.inpainted === undefined}
          className='data-[state=checked]:bg-rose-200 data-[state=unchecked]:bg-neutral-300 [&_[data-slot=switch-thumb]]:data-[state=checked]:bg-rose-500'
        />
        <span>{t('mask.showInpainted')}</span>
      </label>
      <label className='flex items-center gap-2 text-sm'>
        <Switch
          size='sm'
          checked={showSegmentationMask}
          onCheckedChange={setShowSegmentationMask}
          disabled={currentDocument?.segment === undefined}
          className='data-[state=checked]:bg-rose-200 data-[state=unchecked]:bg-neutral-300 [&_[data-slot=switch-thumb]]:data-[state=checked]:bg-rose-500'
        />
        <span>{t('mask.showSegmentationMask')}</span>
      </label>
      <label className='flex items-center gap-2 text-sm'>
        <Switch
          size='sm'
          checked={showBrushLayer}
          onCheckedChange={setShowBrushLayer}
          disabled={currentDocument?.inpainted === undefined}
          className='data-[state=checked]:bg-rose-200 data-[state=unchecked]:bg-neutral-300 [&_[data-slot=switch-thumb]]:data-[state=checked]:bg-rose-500'
        />
        <span>{t('mask.showBrushLayer')}</span>
      </label>
      <label className='flex items-center gap-2 text-sm'>
        <Switch
          size='sm'
          checked={showTextBlocksOverlay}
          onCheckedChange={setShowTextBlocksOverlay}
          disabled={currentDocument?.textBlocks === undefined}
          className='data-[state=checked]:bg-rose-200 data-[state=unchecked]:bg-neutral-300 [&_[data-slot=switch-thumb]]:data-[state=checked]:bg-rose-500'
        />
        <span>{t('mask.showTextBlocks')}</span>
      </label>
      <Separator className='my-1' />
      <div className='flex gap-2'>
        <Tooltip delayDuration={1000}>
          <TooltipTrigger asChild>
            <Button
              variant='outline'
              onClick={detect}
              className='w-full font-semibold'
            >
              {t('processing.detect')}
            </Button>
          </TooltipTrigger>
          <TooltipContent side='bottom' sideOffset={6}>
            {t('processing.detectTooltip')}
          </TooltipContent>
        </Tooltip>
        <Tooltip delayDuration={1000}>
          <TooltipTrigger asChild>
            <Button
              variant='outline'
              onClick={ocr}
              className='w-full font-semibold'
            >
              {t('processing.ocr')}
            </Button>
          </TooltipTrigger>
          <TooltipContent side='bottom' sideOffset={6}>
            {t('processing.ocrTooltip')}
          </TooltipContent>
        </Tooltip>
      </div>
      <div className='flex'>
        <Tooltip delayDuration={1000}>
          <TooltipTrigger asChild>
            <Button
              variant='outline'
              onClick={inpaint}
              className='w-full font-semibold'
            >
              {t('mask.inpaint')}
            </Button>
          </TooltipTrigger>
          <TooltipContent side='bottom' sideOffset={6}>
            {t('mask.inpaintTooltip')}
          </TooltipContent>
        </Tooltip>
      </div>
    </div>
  )
}
