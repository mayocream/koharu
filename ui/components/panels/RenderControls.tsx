'use client'

import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { useAppStore } from '@/lib/store'
import { useTextBlocks } from '@/hooks/useTextBlocks'
import { RenderEffect, RgbaColor, TextStyle } from '@/types'
import { Switch } from '@/components/ui/switch'
import { Button } from '@/components/ui/button'
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

const DEFAULT_COLOR: RgbaColor = [0, 0, 0, 255]
const DEFAULT_FONT_FAMILIES = ['Arial']

const clampByte = (value: number) =>
  Math.max(0, Math.min(255, Math.round(value)))

const colorToHex = (color: RgbaColor) =>
  `#${color
    .slice(0, 3)
    .map((value) => value.toString(16).padStart(2, '0'))
    .join('')}`

const hexToColor = (value: string, alpha: number): RgbaColor => {
  const normalized = value.replace('#', '')
  if (normalized.length !== 6) {
    return [0, 0, 0, clampByte(alpha)]
  }

  const r = Number.parseInt(normalized.slice(0, 2), 16)
  const g = Number.parseInt(normalized.slice(2, 4), 16)
  const b = Number.parseInt(normalized.slice(4, 6), 16)

  if ([r, g, b].some((channel) => Number.isNaN(channel))) {
    return [0, 0, 0, clampByte(alpha)]
  }

  return [r, g, b, clampByte(alpha)]
}

const uniqueStrings = (values: string[]) => {
  const seen = new Set<string>()
  return values.filter((value) => {
    if (!value || seen.has(value)) return false
    seen.add(value)
    return true
  })
}

export function RenderControls() {
  const {
    showRenderedImage,
    setShowRenderedImage,
    render,
    renderEffect,
    setRenderEffect,
    updateTextBlocks,
    availableFonts,
    fetchAvailableFonts,
  } = useAppStore()
  const { textBlocks, selectedBlockIndex, replaceBlock } = useTextBlocks()
  const { t } = useTranslation()
  const selectedBlock =
    selectedBlockIndex !== undefined
      ? textBlocks[selectedBlockIndex]
      : undefined
  const firstBlock = textBlocks[0]
  const hasBlocks = textBlocks.length > 0
  const fallbackFontFamilies =
    availableFonts.length > 0 ? [availableFonts[0]] : DEFAULT_FONT_FAMILIES
  const fallbackColor = firstBlock?.style?.color ?? DEFAULT_COLOR
  const fontCandidates =
    availableFonts.length > 0
      ? availableFonts
      : [
          ...(selectedBlock?.style?.fontFamilies?.slice(0, 1) ?? []),
          ...DEFAULT_FONT_FAMILIES,
        ]
  const fontOptions = uniqueStrings(fontCandidates)
  const currentFont =
    selectedBlock?.style?.fontFamilies?.[0] ??
    firstBlock?.style?.fontFamilies?.[0] ??
    (hasBlocks ? fallbackFontFamilies[0] : '')
  const currentEffect = selectedBlock?.style?.effect ?? renderEffect
  const currentColor =
    selectedBlock?.style?.color ?? (hasBlocks ? fallbackColor : DEFAULT_COLOR)
  const currentColorHex = colorToHex(currentColor)

  useEffect(() => {
    if (availableFonts.length === 0) {
      fetchAvailableFonts()
    }
  }, [availableFonts.length, fetchAvailableFonts])

  const effects: { value: RenderEffect; label: string }[] = [
    { value: 'normal', label: t('render.effectNormal') },
    { value: 'antique', label: t('render.effectAntique') },
    { value: 'metal', label: t('render.effectMetal') },
    { value: 'manga', label: t('render.effectManga') },
    { value: 'motionBlur', label: t('render.effectMotionBlur') },
  ]

  const buildStyle = (
    style: TextStyle | undefined,
    updates: Partial<TextStyle>,
  ): TextStyle => ({
    fontFamilies:
      updates.fontFamilies ?? style?.fontFamilies ?? fallbackFontFamilies,
    fontSize: updates.fontSize ?? style?.fontSize,
    color: updates.color ?? style?.color ?? fallbackColor,
    effect: updates.effect ?? style?.effect,
  })

  const applyStyleToSelected = (updates: Partial<TextStyle>) => {
    if (selectedBlockIndex === undefined) return false
    const nextStyle = buildStyle(selectedBlock?.style, updates)
    void replaceBlock(selectedBlockIndex, { style: nextStyle })
    return true
  }

  const applyStyleToAll = (updates: Partial<TextStyle>) => {
    if (!hasBlocks) return
    const nextBlocks = textBlocks.map((block) => ({
      ...block,
      style: buildStyle(block.style, updates),
    }))
    void updateTextBlocks(nextBlocks)
  }

  const mergeFontFamilies = (
    nextFont: string,
    current: string[] | undefined,
  ) => {
    const base = current?.length ? current : fallbackFontFamilies
    return [nextFont, ...base.filter((family) => family !== nextFont)]
  }

  return (
    <div className='text-muted-foreground space-y-2 text-xs'>
      <label className='flex items-center gap-2 text-sm'>
        <Switch
          size='sm'
          checked={showRenderedImage}
          onCheckedChange={setShowRenderedImage}
          className='data-[state=checked]:bg-primary/30 data-[state=unchecked]:bg-muted-foreground/30 [&_[data-slot=switch-thumb]]:data-[state=checked]:bg-primary'
        />
        <span>{t('mask.showRendered')}</span>
      </label>
      <div className='space-y-1'>
        <div className='flex items-center justify-between gap-2'>
          <span className='text-muted-foreground text-[11px] font-semibold tracking-wide uppercase'>
            {t('render.fontLabel')}
          </span>
          <span className='border-border bg-card text-muted-foreground rounded border px-1.5 py-0.5 text-[10px] font-semibold'>
            {selectedBlockIndex !== undefined
              ? t('render.fontScopeBlockIndex', {
                  index: selectedBlockIndex + 1,
                })
              : t('render.fontScopeGlobal')}
          </span>
        </div>
        <Select
          value={currentFont}
          onValueChange={(value) => {
            const nextFamilies = mergeFontFamilies(
              value,
              selectedBlock?.style?.fontFamilies,
            )
            if (applyStyleToSelected({ fontFamilies: nextFamilies })) return
            if (!hasBlocks) return
            const nextBlocks = textBlocks.map((block) => ({
              ...block,
              style: buildStyle(block.style, {
                fontFamilies: mergeFontFamilies(
                  value,
                  block.style?.fontFamilies,
                ),
              }),
            }))
            void updateTextBlocks(nextBlocks)
          }}
          disabled={!hasBlocks || fontOptions.length === 0}
        >
          <SelectTrigger
            className='w-full'
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
      </div>
      <div className='space-y-1'>
        <div className='text-muted-foreground text-[11px] font-semibold tracking-wide uppercase'>
          {t('render.effectLabel')}
        </div>
        <Select
          value={currentEffect}
          onValueChange={(value) => {
            const nextEffect = value as RenderEffect
            if (applyStyleToSelected({ effect: nextEffect })) return
            setRenderEffect(nextEffect)
          }}
        >
          <SelectTrigger className='w-full'>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {effects.map((effect) => (
              <SelectItem key={effect.value} value={effect.value}>
                {effect.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      <div className='space-y-1'>
        <div className='text-muted-foreground text-[11px] font-semibold tracking-wide uppercase'>
          {t('render.fontColorLabel')}
        </div>
        <div className='border-border bg-card inline-flex w-full items-center justify-between gap-3 rounded border px-2 py-1 text-sm'>
          <input
            type='color'
            value={currentColorHex}
            disabled={!hasBlocks}
            onChange={(event) => {
              const nextColor = hexToColor(
                event.target.value,
                currentColor[3] ?? 255,
              )
              if (applyStyleToSelected({ color: nextColor })) return
              applyStyleToAll({ color: nextColor })
            }}
            className='h-6 w-6 cursor-pointer appearance-none border-none p-0 disabled:cursor-not-allowed disabled:opacity-60'
          />
          <span className='text-muted-foreground font-mono text-[11px]'>
            {currentColorHex.toUpperCase()}
          </span>
        </div>
      </div>
      <div className='col flex'>
        <Tooltip delayDuration={1000}>
          <TooltipTrigger asChild>
            <Button
              variant='outline'
              onClick={render}
              className='w-full font-semibold'
            >
              {t('llm.render')}
            </Button>
          </TooltipTrigger>
          <TooltipContent side='bottom' sideOffset={6}>
            {t('llm.renderTooltip')}
          </TooltipContent>
        </Tooltip>
      </div>
    </div>
  )
}
