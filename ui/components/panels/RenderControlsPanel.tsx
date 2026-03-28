'use client'

import type { ComponentType } from 'react'
import { useTranslation } from 'react-i18next'
import {
  AlignCenterIcon,
  AlignLeftIcon,
  AlignRightIcon,
  BoldIcon,
  ItalicIcon,
  MinusIcon,
  PlusIcon,
  SquareIcon,
} from 'lucide-react'
import { useTextBlocks } from '@/hooks/useTextBlocks'
import {
  RenderEffect,
  RenderStroke,
  RgbaColor,
  TextAlign,
  TextStyle,
} from '@/types'
import type { FontFaceInfo } from '@/lib/protocol'
import { Button } from '@/components/ui/button'
import { ColorPicker } from '@/components/ui/color-picker'
import { Input } from '@/components/ui/input'
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
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'
import { useFontsQuery } from '@/lib/query/hooks'
import { useTextBlockMutations } from '@/lib/query/mutations'
import { cn } from '@/lib/utils'

const DEFAULT_COLOR: RgbaColor = [0, 0, 0, 255]
const DEFAULT_FONT_FACES: FontFaceInfo[] = [
  {
    familyName: 'Arial',
    postScriptName: 'ArialMT',
  },
]
const DEFAULT_EFFECT: RenderEffect = {
  italic: false,
  bold: false,
}
const DEFAULT_STROKE: RenderStroke = {
  enabled: true,
  color: [255, 255, 255, 255],
  widthPx: undefined,
}
const DEFAULT_STROKE_WIDTH = 1.6
const MIN_STROKE_WIDTH = 0.2
const MAX_STROKE_WIDTH = 24
const STROKE_WIDTH_STEP = 0.1
const DEFAULT_FONT_SIZE = 24
const MIN_FONT_SIZE = 4
const MAX_FONT_SIZE = 200
const LATIN_ONLY_PATTERN =
  /^[\p{Script=Latin}\p{Script=Common}\p{Script=Inherited}]*$/u

const clampByte = (value: number) =>
  Math.max(0, Math.min(255, Math.round(value)))

const clampStrokeWidth = (value: number) =>
  Number(
    Math.max(MIN_STROKE_WIDTH, Math.min(MAX_STROKE_WIDTH, value)).toFixed(1),
  )

const clampFontSize = (value: number) =>
  Math.max(MIN_FONT_SIZE, Math.min(MAX_FONT_SIZE, Math.round(value)))

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

const uniqueFontFaces = (values: FontFaceInfo[]) => {
  const seen = new Set<string>()
  return values.filter((value) => {
    if (!value.postScriptName || seen.has(value.postScriptName)) return false
    seen.add(value.postScriptName)
    return true
  })
}

const findFontFace = (fonts: FontFaceInfo[], value?: string) => {
  if (!value) return undefined
  return fonts.find(
    (font) =>
      font.postScriptName === value ||
      font.familyName === value ||
      font.familyName.trim() === value.trim(),
  )
}

const normalizeFontValue = (fonts: FontFaceInfo[], value?: string) =>
  findFontFace(fonts, value)?.postScriptName ?? value

const fallbackFontFace = (value?: string): FontFaceInfo | undefined => {
  const normalized = value?.trim()
  if (!normalized) return undefined
  return {
    familyName: normalized,
    postScriptName: normalized,
  }
}

const normalizeEffect = (effect?: Partial<RenderEffect>): RenderEffect => ({
  italic: effect?.italic ?? false,
  bold: effect?.bold ?? false,
})

const normalizeStroke = (stroke?: Partial<RenderStroke>): RenderStroke => ({
  enabled: stroke?.enabled ?? true,
  color: stroke?.color ?? DEFAULT_STROKE.color,
  widthPx: stroke?.widthPx,
})

const resolveStyleColor = (
  style: TextStyle | undefined,
  block:
    | {
        fontPrediction?: {
          text_color: [number, number, number]
        }
      }
    | undefined,
  fallbackColor: RgbaColor,
): RgbaColor =>
  style?.color ??
  (block?.fontPrediction?.text_color
    ? [
        block.fontPrediction.text_color[0],
        block.fontPrediction.text_color[1],
        block.fontPrediction.text_color[2],
        255,
      ]
    : fallbackColor)

const resolveEffectiveTextAlign = (
  block:
    | {
        style?: TextStyle
        translation?: string
      }
    | undefined,
): TextAlign => {
  if (block?.style?.textAlign) {
    return block.style.textAlign
  }

  if (block?.translation && LATIN_ONLY_PATTERN.test(block.translation)) {
    return 'center'
  }

  return 'left'
}

export function RenderControlsPanel() {
  const renderEffect = useEditorUiStore((state) => state.renderEffect)
  const renderStroke = useEditorUiStore((state) => state.renderStroke)
  const setRenderEffect = useEditorUiStore((state) => state.setRenderEffect)
  const setRenderStroke = useEditorUiStore((state) => state.setRenderStroke)
  const selectedBlockIndices = useEditorUiStore(
    (state) => state.selectedBlockIndices,
  )
  const { updateTextBlocks } = useTextBlockMutations()
  const { data: availableFonts = [] } = useFontsQuery()
  const fontFamily = usePreferencesStore((state) => state.fontFamily)
  const setFontFamily = usePreferencesStore((state) => state.setFontFamily)
  const {
    textBlocks,
    selectedBlockIndex,
    replaceBlock,
    replaceMultipleBlocks,
  } = useTextBlocks()
  const { t } = useTranslation()
  const selectedBlock =
    selectedBlockIndex !== undefined
      ? textBlocks[selectedBlockIndex]
      : undefined
  const firstBlock = textBlocks[0]
  const hasBlocks = textBlocks.length > 0
  const isMultiSelect = selectedBlockIndices.length > 1
  const fontCandidates = uniqueFontFaces(
    [
      ...availableFonts,
      ...(fontFamily ? [fallbackFontFace(fontFamily)] : []),
      ...(selectedBlock?.style?.fontFamilies
        ?.slice(0, 1)
        ?.map(fallbackFontFace) ?? []),
      ...(firstBlock?.style?.fontFamilies?.slice(0, 1)?.map(fallbackFontFace) ??
        []),
      ...DEFAULT_FONT_FACES,
    ].filter((value): value is FontFaceInfo => !!value),
  )
  const fallbackFontFaces =
    fontCandidates.length > 0 ? fontCandidates : DEFAULT_FONT_FACES
  const fallbackColor = firstBlock?.style?.color ?? DEFAULT_COLOR
  const fontOptions = fontCandidates
  const currentFontCandidate =
    selectedBlock?.style?.fontFamilies?.[0] ??
    fontFamily ??
    firstBlock?.style?.fontFamilies?.[0] ??
    (hasBlocks ? fallbackFontFaces[0]?.postScriptName : '')
  const currentFontFace =
    findFontFace(fontOptions, currentFontCandidate) ??
    fallbackFontFace(currentFontCandidate)
  const currentFont = currentFontFace?.postScriptName ?? ''
  const currentFontFamilyName = currentFontFace?.familyName
  const currentEffect = normalizeEffect(
    selectedBlock?.style?.effect ?? renderEffect,
  )
  const currentStroke = normalizeStroke(
    selectedBlock?.style?.stroke ?? renderStroke,
  )
  const currentColor =
    selectedBlock?.style?.color ?? (hasBlocks ? fallbackColor : DEFAULT_COLOR)
  const currentColorHex = colorToHex(currentColor)
  const currentStrokeColorHex = colorToHex(currentStroke.color)
  const currentStrokeWidth = currentStroke.widthPx ?? DEFAULT_STROKE_WIDTH
  const currentFontSize =
    selectedBlock?.style?.fontSize ??
    selectedBlock?.detectedFontSizePx ??
    selectedBlock?.fontPrediction?.font_size_px ??
    DEFAULT_FONT_SIZE
  const fontLabel = t('render.fontLabel')
  const effectLabel = t('render.effectLabel')
  const strokeLabel = t('render.effectBorder')
  const strokeColorLabel = t('render.strokeColorLabel')
  const strokeWidthLabel = t('render.strokeWidthLabel')
  const alignLabel = t('render.alignLabel')
  const currentTextAlign = resolveEffectiveTextAlign(
    selectedBlock ?? firstBlock,
  )
  const scopeLabel = isMultiSelect
    ? t('render.fontScopeMulti', {
        count: selectedBlockIndices.length,
        defaultValue: `${selectedBlockIndices.length} blocks`,
      })
    : selectedBlockIndex !== undefined
      ? t('render.fontScopeBlockIndex', {
          index: selectedBlockIndex + 1,
        })
      : t('render.fontScopeGlobal')
  const scopeToneClass = isMultiSelect
    ? 'border-sky-400/20 bg-sky-400/10 text-sky-600'
    : selectedBlockIndex !== undefined
      ? 'border-primary/20 bg-primary/10 text-primary'
      : 'border-border/60 bg-muted text-muted-foreground'

  const buildStyle = (
    block:
      | {
          style?: TextStyle
          fontPrediction?: {
            text_color: [number, number, number]
          }
        }
      | undefined,
    style: TextStyle | undefined,
    updates: Partial<TextStyle>,
  ): TextStyle => ({
    fontFamilies: updates.fontFamilies ?? style?.fontFamilies ?? [],
    fontSize: updates.fontSize ?? style?.fontSize,
    color: updates.color ?? resolveStyleColor(style, block, fallbackColor),
    effect: updates.effect ?? style?.effect,
    stroke: updates.stroke ?? style?.stroke,
    textAlign: updates.textAlign ?? style?.textAlign,
  })

  const applyStyleToSelected = (updates: Partial<TextStyle>) => {
    // Multi-select: apply to all selected blocks
    if (isMultiSelect) {
      const nextBlocks = textBlocks.map((block, idx) =>
        selectedBlockIndices.includes(idx)
          ? { ...block, style: buildStyle(block, block.style, updates) }
          : block,
      )
      void updateTextBlocks(nextBlocks)
      return true
    }
    if (selectedBlockIndex === undefined) return false
    const nextStyle = buildStyle(selectedBlock, selectedBlock?.style, updates)
    void replaceBlock(selectedBlockIndex, { style: nextStyle })
    return true
  }

  const applyStyleToAll = (updates: Partial<TextStyle>) => {
    if (!hasBlocks) return
    const nextBlocks = textBlocks.map((block) => ({
      ...block,
      style: buildStyle(block, block.style, updates),
    }))
    void updateTextBlocks(nextBlocks)
  }

  const mergeFontFamilies = (
    nextFont: string,
    current: string[] | undefined,
  ) => {
    const base = (
      current?.length
        ? current
        : fallbackFontFaces.map((font) => font.postScriptName)
    ).map((family) => normalizeFontValue(fontOptions, family) ?? family)
    return [nextFont, ...base.filter((family) => family !== nextFont)]
  }

  const applyStrokeSetting = (nextStroke: RenderStroke) => {
    const normalized = normalizeStroke(nextStroke)
    if (applyStyleToSelected({ stroke: normalized })) return
    setRenderStroke(normalized)
  }

  const updateStrokeWidth = (value: number) => {
    applyStrokeSetting({
      ...currentStroke,
      widthPx: clampStrokeWidth(value),
    })
  }

  const updateFontSize = (value: number) => {
    const clamped = clampFontSize(value)
    if (applyStyleToSelected({ fontSize: clamped })) return
    // No global fontSize fallback — it requires a selected block
  }

  const effectItems: {
    key: keyof RenderEffect
    label: string
    Icon: ComponentType<{ className?: string }>
  }[] = [
    { key: 'italic', label: t('render.effectItalic'), Icon: ItalicIcon },
    { key: 'bold', label: t('render.effectBold'), Icon: BoldIcon },
  ]

  const textAlignItems: {
    value: TextAlign
    label: string
    Icon: ComponentType<{ className?: string }>
  }[] = [
    {
      value: 'left',
      label: t('render.alignLeft'),
      Icon: AlignLeftIcon,
    },
    {
      value: 'center',
      label: t('render.alignCenter'),
      Icon: AlignCenterIcon,
    },
    {
      value: 'right',
      label: t('render.alignRight'),
      Icon: AlignRightIcon,
    },
  ]

  return (
    <div className='flex w-full min-w-0 flex-col gap-1.5'>
      <div className='flex items-center justify-end'>
        <span
          data-testid='render-scope-indicator'
          className={cn(
            'rounded-full border px-2 py-0.5 text-[10px] font-medium tracking-wide uppercase',
            scopeToneClass,
          )}
        >
          {scopeLabel}
        </span>
      </div>

      <div className='grid w-full min-w-0 grid-cols-[3.5rem_minmax(0,1fr)] items-center gap-1.5'>
        <span className='text-muted-foreground text-[10px] font-medium tracking-wide uppercase'>
          {fontLabel}
        </span>

        <div className='flex min-w-0 items-center gap-1.5'>
          <div className='min-w-0 flex-1'>
            <Select
              value={currentFont}
              onValueChange={(value) => {
                const nextFamilies = mergeFontFamilies(
                  value,
                  selectedBlock?.style?.fontFamilies,
                )
                if (applyStyleToSelected({ fontFamilies: nextFamilies })) return
                setFontFamily(value)
                if (!hasBlocks) return
                const nextBlocks = textBlocks.map((block) => ({
                  ...block,
                  style: buildStyle(block, block.style, {
                    fontFamilies: mergeFontFamilies(
                      value,
                      block.style?.fontFamilies,
                    ),
                  }),
                }))
                void updateTextBlocks(nextBlocks)
              }}
              disabled={fontOptions.length === 0}
            >
              <SelectTrigger
                data-testid='render-font-select'
                size='sm'
                className='h-7 w-full min-w-0 text-xs'
                style={
                  currentFontFamilyName
                    ? { fontFamily: currentFontFamilyName }
                    : undefined
                }
              >
                <SelectValue placeholder={t('render.fontPlaceholder')} />
              </SelectTrigger>
              <SelectContent position='popper'>
                {fontOptions.map((font, index) => (
                  <SelectItem
                    key={font.postScriptName}
                    value={font.postScriptName}
                    style={{ fontFamily: font.familyName }}
                    data-testid={`render-font-option-${index}`}
                  >
                    {font.familyName}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <Tooltip>
            <TooltipTrigger asChild>
              <div>
                <ColorPicker
                  value={currentColorHex}
                  disabled={!hasBlocks}
                  triggerTestId='render-color-trigger'
                  pickerTestId='render-color-picker'
                  swatchTestId='render-color-swatch'
                  inputTestId='render-color-input'
                  pickButtonTestId='render-color-pick'
                  onChange={(hex) => {
                    const nextColor = hexToColor(hex, currentColor[3] ?? 255)
                    if (applyStyleToSelected({ color: nextColor })) return
                    applyStyleToAll({ color: nextColor })
                  }}
                  className='h-7 w-7'
                />
              </div>
            </TooltipTrigger>
            <TooltipContent side='bottom' sideOffset={4}>
              {t('render.fontColorLabel')}
            </TooltipContent>
          </Tooltip>
        </div>
      </div>

      {/* Font Size */}
      <div className='grid w-full min-w-0 grid-cols-[3.5rem_minmax(0,1fr)] items-center gap-1.5'>
        <span className='text-muted-foreground text-[10px] font-medium tracking-wide uppercase'>
          {t('render.fontSizeLabel')}
        </span>

        <div className='flex min-w-0 items-center gap-1'>
          <Tooltip>
            <TooltipTrigger asChild>
              <div className='border-input bg-background flex w-auto min-w-0 shrink-0 items-center rounded-md border shadow-xs'>
                <Button
                  type='button'
                  variant='ghost'
                  size='icon-sm'
                  aria-label='Decrease font size'
                  className='size-7 rounded-r-none border-r'
                  disabled={!hasBlocks}
                  onClick={() => updateFontSize(currentFontSize - 1)}
                >
                  <MinusIcon className='size-3' />
                </Button>

                <Input
                  type='number'
                  step='1'
                  min={String(MIN_FONT_SIZE)}
                  max={String(MAX_FONT_SIZE)}
                  inputMode='numeric'
                  className='h-7 w-14 min-w-0 [appearance:textfield] rounded-none border-0 px-1.5 text-center text-[11px] shadow-none focus-visible:ring-0 [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none'
                  data-testid='render-font-size'
                  disabled={!hasBlocks}
                  value={
                    Number.isFinite(currentFontSize)
                      ? Math.round(currentFontSize)
                      : DEFAULT_FONT_SIZE
                  }
                  onChange={(event) => {
                    const parsed = Number.parseInt(event.target.value, 10)
                    if (!Number.isFinite(parsed)) return
                    updateFontSize(parsed)
                  }}
                />

                <Button
                  type='button'
                  variant='ghost'
                  size='icon-sm'
                  aria-label='Increase font size'
                  className='size-7 rounded-l-none border-l'
                  disabled={!hasBlocks}
                  onClick={() => updateFontSize(currentFontSize + 1)}
                >
                  <PlusIcon className='size-3' />
                </Button>
              </div>
            </TooltipTrigger>
            <TooltipContent side='bottom' sideOffset={4}>
              {t('render.fontSizeTooltip')}
            </TooltipContent>
          </Tooltip>
        </div>
      </div>

      <div className='grid w-full min-w-0 grid-cols-[3.5rem_minmax(0,1fr)] items-center gap-1.5'>
        <span className='text-muted-foreground text-[10px] font-medium tracking-wide uppercase'>
          {effectLabel}
        </span>

        <div className='flex min-w-0 flex-wrap items-center gap-1'>
          {effectItems.map((item) => {
            const active = currentEffect[item.key]
            const Icon = item.Icon
            return (
              <Tooltip key={item.key}>
                <TooltipTrigger asChild>
                  <Button
                    variant='outline'
                    size='icon-sm'
                    aria-label={item.label}
                    data-testid={`render-effect-toggle-${item.key}`}
                    className={cn(
                      'size-7',
                      active &&
                        'bg-primary text-primary-foreground border-primary hover:bg-primary/90',
                    )}
                    onClick={() => {
                      const nextEffect = {
                        ...DEFAULT_EFFECT,
                        ...currentEffect,
                        [item.key]: !active,
                      }
                      if (applyStyleToSelected({ effect: nextEffect })) return
                      setRenderEffect(nextEffect)
                    }}
                  >
                    <Icon className='size-3.5' />
                  </Button>
                </TooltipTrigger>
                <TooltipContent side='bottom' sideOffset={4}>
                  {item.label}
                </TooltipContent>
              </Tooltip>
            )
          })}
        </div>
      </div>

      <div className='grid w-full min-w-0 grid-cols-[3.5rem_minmax(0,1fr)] items-center gap-1.5'>
        <span className='text-muted-foreground text-[10px] font-medium tracking-wide uppercase'>
          {alignLabel}
        </span>

        <div className='flex min-w-0 flex-wrap items-center gap-1'>
          {textAlignItems.map((item) => {
            const active = currentTextAlign === item.value
            const Icon = item.Icon
            return (
              <Tooltip key={item.value}>
                <TooltipTrigger asChild>
                  <Button
                    variant='outline'
                    size='icon-sm'
                    aria-label={item.label}
                    data-testid={`render-align-${item.value}`}
                    disabled={!hasBlocks}
                    className={cn(
                      'size-7',
                      active &&
                        'bg-primary text-primary-foreground border-primary hover:bg-primary/90',
                    )}
                    onClick={() => {
                      if (applyStyleToSelected({ textAlign: item.value }))
                        return
                      applyStyleToAll({ textAlign: item.value })
                    }}
                  >
                    <Icon className='size-3.5' />
                  </Button>
                </TooltipTrigger>
                <TooltipContent side='bottom' sideOffset={4}>
                  {item.label}
                </TooltipContent>
              </Tooltip>
            )
          })}
        </div>
      </div>

      <div className='grid w-full min-w-0 grid-cols-[3.5rem_minmax(0,1fr)] items-center gap-1.5'>
        <span className='text-muted-foreground text-[10px] font-medium tracking-wide uppercase'>
          {strokeLabel}
        </span>

        <div className='flex min-w-0 flex-wrap items-center gap-1'>
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant='outline'
                size='icon-sm'
                aria-label={strokeLabel}
                data-testid='render-stroke-enable'
                className={cn(
                  'size-7 shrink-0',
                  currentStroke.enabled &&
                    'bg-primary text-primary-foreground border-primary hover:bg-primary/90',
                )}
                onClick={() =>
                  applyStrokeSetting({
                    ...currentStroke,
                    enabled: !currentStroke.enabled,
                  })
                }
              >
                <SquareIcon className='size-3.5' />
              </Button>
            </TooltipTrigger>
            <TooltipContent side='bottom' sideOffset={4}>
              {strokeLabel}
            </TooltipContent>
          </Tooltip>

          <Tooltip>
            <TooltipTrigger asChild>
              <div>
                <ColorPicker
                  value={currentStrokeColorHex}
                  disabled={!hasBlocks}
                  triggerTestId='render-stroke-color-trigger'
                  pickerTestId='render-stroke-color-picker'
                  swatchTestId='render-stroke-color-swatch'
                  inputTestId='render-stroke-color-input'
                  pickButtonTestId='render-stroke-color-pick'
                  onChange={(hex) => {
                    applyStrokeSetting({
                      ...currentStroke,
                      color: hexToColor(hex, currentStroke.color[3] ?? 255),
                    })
                  }}
                  className='h-7 w-7'
                />
              </div>
            </TooltipTrigger>
            <TooltipContent side='bottom' sideOffset={4}>
              {strokeColorLabel}
            </TooltipContent>
          </Tooltip>

          <Tooltip>
            <TooltipTrigger asChild>
              <div className='border-input bg-background flex w-auto min-w-0 shrink-0 items-center rounded-md border shadow-xs'>
                <Button
                  type='button'
                  variant='ghost'
                  size='icon-sm'
                  aria-label={`${strokeWidthLabel} -`}
                  className='size-7 rounded-r-none border-r'
                  onClick={() =>
                    updateStrokeWidth(currentStrokeWidth - STROKE_WIDTH_STEP)
                  }
                >
                  <MinusIcon className='size-3' />
                </Button>

                <Input
                  type='number'
                  step={String(STROKE_WIDTH_STEP)}
                  min={String(MIN_STROKE_WIDTH)}
                  max={String(MAX_STROKE_WIDTH)}
                  inputMode='decimal'
                  className='h-7 w-14 min-w-0 [appearance:textfield] rounded-none border-0 px-1.5 text-center text-[11px] shadow-none focus-visible:ring-0 [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none'
                  data-testid='render-stroke-width'
                  value={
                    Number.isFinite(currentStrokeWidth)
                      ? currentStrokeWidth
                      : DEFAULT_STROKE_WIDTH
                  }
                  onChange={(event) => {
                    const parsed = Number.parseFloat(event.target.value)
                    if (!Number.isFinite(parsed)) return
                    updateStrokeWidth(parsed)
                  }}
                />

                <Button
                  type='button'
                  variant='ghost'
                  size='icon-sm'
                  aria-label={`${strokeWidthLabel} +`}
                  className='size-7 rounded-l-none border-l'
                  onClick={() =>
                    updateStrokeWidth(currentStrokeWidth + STROKE_WIDTH_STEP)
                  }
                >
                  <PlusIcon className='size-3' />
                </Button>
              </div>
            </TooltipTrigger>
            <TooltipContent side='bottom' sideOffset={4}>
              {strokeWidthLabel}
            </TooltipContent>
          </Tooltip>
        </div>
      </div>
    </div>
  )
}
