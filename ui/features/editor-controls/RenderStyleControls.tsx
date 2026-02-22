'use client'

import { useTranslation } from 'react-i18next'
import { PaletteIcon } from 'lucide-react'
import { useAppStore } from '@/lib/store'
import { useRenderStyleControls } from '@/features/editor-controls/useRenderStyleControls'
import type { RenderEffect } from '@/types'
import { Button } from '@/components/ui/button'
import { ColorPicker } from '@/components/ui/color-picker'
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { cn } from '@/lib/utils'

type RenderStyleControlsVariant = 'toolbar' | 'panel' | 'popover'

type RenderStyleControlsProps = {
  variant?: RenderStyleControlsVariant
  className?: string
}

const EFFECT_LABEL_KEYS: Record<RenderEffect, string> = {
  normal: 'render.effectNormal',
  antique: 'render.effectAntique',
  metal: 'render.effectMetal',
  manga: 'render.effectManga',
  motionBlur: 'render.effectMotionBlur',
}

export function RenderStyleControls({
  variant = 'popover',
  className,
}: RenderStyleControlsProps) {
  const { t } = useTranslation()
  const render = useAppStore((state) => state.render)
  const {
    hasBlocks,
    selectedBlockIndex,
    fontOptions,
    currentFont,
    currentEffect,
    currentColorHex,
    effects,
    setFont,
    setColor,
    setEffect,
  } = useRenderStyleControls()

  const compact = variant === 'toolbar' || variant === 'popover'

  if (compact) {
    return (
      <div className={cn('flex items-center gap-2', className)}>
        <Select
          value={currentFont}
          onValueChange={setFont}
          disabled={fontOptions.length === 0}
        >
          <SelectTrigger
            size='sm'
            className='h-8 w-32 text-sm'
            style={currentFont ? { fontFamily: currentFont } : undefined}
          >
            <SelectValue placeholder={t('render.fontPlaceholder')} />
          </SelectTrigger>
          <SelectContent position='popper'>
            {fontOptions.map((font) => (
              <SelectItem key={font} value={font} style={{ fontFamily: font }}>
                {font}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        <Tooltip>
          <TooltipTrigger asChild>
            <div>
              <ColorPicker
                value={currentColorHex}
                disabled={!hasBlocks}
                onChange={setColor}
                className='h-8 w-8'
              />
            </div>
          </TooltipTrigger>
          <TooltipContent side='bottom' sideOffset={4}>
            {t('render.fontColorLabel')}
          </TooltipContent>
        </Tooltip>

        <Select value={currentEffect} onValueChange={setEffect}>
          <SelectTrigger size='sm' className='h-8 w-28 text-sm'>
            <SelectValue />
          </SelectTrigger>
          <SelectContent position='popper'>
            {effects.map((effect) => (
              <SelectItem key={effect} value={effect}>
                {t(EFFECT_LABEL_KEYS[effect])}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
    )
  }

  return (
    <div className={cn('space-y-2 text-xs', className)}>
      <div className='flex items-center gap-1.5'>
        <Select
          value={currentFont}
          onValueChange={setFont}
          disabled={!hasBlocks || fontOptions.length === 0}
        >
          <SelectTrigger
            className='flex-1'
            style={currentFont ? { fontFamily: currentFont } : undefined}
          >
            <SelectValue placeholder={t('render.fontPlaceholder')} />
          </SelectTrigger>
          <SelectContent>
            {fontOptions.map((font) => (
              <SelectItem key={font} value={font} style={{ fontFamily: font }}>
                {font}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        <Tooltip>
          <TooltipTrigger asChild>
            <div>
              <ColorPicker
                value={currentColorHex}
                disabled={!hasBlocks}
                onChange={setColor}
              />
            </div>
          </TooltipTrigger>
          <TooltipContent side='left' sideOffset={4}>
            {t('render.fontColorLabel')}
          </TooltipContent>
        </Tooltip>
      </div>

      <Select value={currentEffect} onValueChange={setEffect}>
        <SelectTrigger className='w-full'>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {effects.map((effect) => (
            <SelectItem key={effect} value={effect}>
              {t(EFFECT_LABEL_KEYS[effect])}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      <Button
        variant='outline'
        size='sm'
        onClick={render}
        className='w-full gap-1.5 text-xs'
      >
        <PaletteIcon className='size-3.5' />
        {t('llm.render')}
      </Button>

      {selectedBlockIndex !== undefined && (
        <p className='text-muted-foreground text-center text-[10px]'>
          {t('render.fontScopeBlockIndex', { index: selectedBlockIndex + 1 })}
        </p>
      )}
    </div>
  )
}
