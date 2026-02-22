'use client'

import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  ScanIcon,
  ScanTextIcon,
  Wand2Icon,
  TypeIcon,
  LoaderCircleIcon,
  LanguagesIcon,
  ScanSearchIcon,
  TextIcon,
  EraserIcon,
} from 'lucide-react'
import { Separator } from '@/components/ui/separator'
import { Button } from '@/components/ui/button'
import { useAppStore } from '@/lib/store'
import { cn } from '@/lib/utils'

type ProcessingActionsVariant = 'toolbar' | 'panel'

type ProcessingActionsProps = {
  variant?: ProcessingActionsVariant
  className?: string
}

export function ProcessingActions({
  variant = 'toolbar',
  className,
}: ProcessingActionsProps) {
  const detect = useAppStore((state) => state.detect)
  const ocr = useAppStore((state) => state.ocr)
  const inpaint = useAppStore((state) => state.inpaint)
  const render = useAppStore((state) => state.render)
  const llmReady = useAppStore((state) => state.llmReady)
  const llmGenerate = useAppStore((state) => state.llmGenerate)
  const { t } = useTranslation()
  const [generating, setGenerating] = useState(false)

  const handleTranslate = async () => {
    setGenerating(true)
    try {
      await llmGenerate(null)
    } catch (error) {
      console.error(error)
    } finally {
      setGenerating(false)
    }
  }

  if (variant === 'panel') {
    return (
      <div className={cn('flex gap-1', className)}>
        <Button
          variant='outline'
          size='sm'
          onClick={detect}
          className='flex-1 gap-1.5 text-xs'
        >
          <ScanSearchIcon className='size-3.5' />
          {t('processing.detect')}
        </Button>
        <Button
          variant='outline'
          size='sm'
          onClick={ocr}
          className='flex-1 gap-1.5 text-xs'
        >
          <TextIcon className='size-3.5' />
          {t('processing.ocr')}
        </Button>
        <Button
          variant='outline'
          size='sm'
          onClick={inpaint}
          className='flex-1 gap-1.5 text-xs'
        >
          <EraserIcon className='size-3.5' />
          {t('mask.inpaint')}
        </Button>
      </div>
    )
  }

  return (
    <div className={cn('flex items-center gap-0.5', className)}>
      <Button variant='ghost' size='xs' onClick={detect}>
        <ScanIcon className='size-4' />
        {t('processing.detect')}
      </Button>

      <Separator orientation='vertical' className='mx-0.5 h-4' />

      <Button variant='ghost' size='xs' onClick={ocr}>
        <ScanTextIcon className='size-4' />
        {t('processing.ocr')}
      </Button>

      <Separator orientation='vertical' className='mx-0.5 h-4' />

      <Button
        variant='ghost'
        size='xs'
        onClick={() => void handleTranslate()}
        disabled={!llmReady || generating}
      >
        {generating ? (
          <LoaderCircleIcon className='size-4 animate-spin' />
        ) : (
          <LanguagesIcon className='size-4' />
        )}
        {t('llm.generate')}
      </Button>

      <Separator orientation='vertical' className='mx-0.5 h-4' />

      <Button variant='ghost' size='xs' onClick={inpaint}>
        <Wand2Icon className='size-4' />
        {t('mask.inpaint')}
      </Button>

      <Separator orientation='vertical' className='mx-0.5 h-4' />

      <Button variant='ghost' size='xs' onClick={render}>
        <TypeIcon className='size-4' />
        {t('llm.render')}
      </Button>
    </div>
  )
}
