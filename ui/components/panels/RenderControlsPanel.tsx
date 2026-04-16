'use client'

import { useQueryClient } from '@tanstack/react-query'
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
import { type ComponentType, useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { ColorPicker } from '@/components/ui/color-picker'
import { FontSelect } from '@/components/ui/font-select'
import { Input } from '@/components/ui/input'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { useTextBlocks } from '@/hooks/useTextBlocks'
import { getGetDocumentQueryKey, updateDocumentStyle } from '@/lib/api/documents/documents'
import type { FontFaceInfo } from '@/lib/api/schemas'
import { useListFonts, useGetGoogleFontsCatalog } from '@/lib/api/system/system'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'
import { cn } from '@/lib/utils'
import { RenderEffect, RenderStroke, RgbaColor, TextAlign, TextStyle } from '@/types'

const DEFAULT_COLOR: RgbaColor = [0, 0, 0, 255]
const DEFAULT_FONT_FACES: FontFaceInfo[] = [
  {
    familyName: 'Arial',
    postScriptName: 'ArialMT',
    source: 'system',
    cached: true,
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
const LATIN_ONLY_PATTERN = /^[\p{Script=Latin}\p{Script=Common}\p{Script=Inherited}]*$/u

const clampByte = (value: number) => Math.max(0, Math.min(255, Math.round(value)))

const clampStrokeWidth = (value: number) =>
  Number(Math.max(MIN_STROKE_WIDTH, Math.min(MAX_STROKE_WIDTH, value)).toFixed(1))

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
    source: 'system',
    cached: true,
  }
}

const hasExplicitFontFamilies = (style?: TextStyle) => (style?.fontFamilies?.length ?? 0) > 0

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
  const { data: availableFonts = [] } = useListFonts()
  const { data: catalog } = useGetGoogleFontsCatalog()
  const sortedFonts = useMemo(() => {
    if (!availableFonts) return []
    const recommended = new Set(catalog?.recommended ?? [])
    return [...availableFonts].sort((a, b) => {
      const aRec = recommended.has(a.familyName) ? 0 : 1
      const bRec = recommended.has(b.familyName) ? 0 : 1
      if (aRec !== bRec) return aRec - bRec
      return a.familyName.localeCompare(b.familyName)
    })
  }, [availableFonts, catalog])
  const {
    document: currentDocument,
    textBlocks,
    selectedBlockIndex,
    replaceBlock,
    updateTextBlocks,
  } = useTextBlocks()
  const documentId = currentDocument?.id
  const appDefaultFont = usePreferencesStore((state) => state.defaultFont)
  const documentFont = currentDocument?.style?.defaultFont ?? appDefaultFont
  const queryClient = useQueryClient()
  const { t } = useTranslation()
  const selectedBlock =
    selectedBlockIndex !== undefined ? textBlocks[selectedBlockIndex] : undefined
  const selectedBlockHasExplicitFont = hasExplicitFontFamilies(selectedBlock?.style)
  const firstBlock = textBlocks[0]
  const hasBlocks = textBlocks.length > 0
  const fontCandidates = uniqueFontFaces(
    [
      ...sortedFonts,
      ...(documentFont ? [fallbackFontFace(documentFont)] : []),
      ...(selectedBlock?.style?.fontFamilies?.slice(0, 1)?.map(fallbackFontFace) ?? []),
      ...(firstBlock?.style?.fontFamilies?.slice(0, 1)?.map(fallbackFontFace) ?? []),
      ...DEFAULT_FONT_FACES,
    ].filter((value): value is FontFaceInfo => !!value),
  )
  const fallbackFontFaces = fontCandidates.length > 0 ? fontCandidates : DEFAULT_FONT_FACES
  const fallbackColor = firstBlock?.style?.color ?? DEFAULT_COLOR
  const fontOptions = fontCandidates
  const currentFontCandidate =
    selectedBlock?.style?.fontFamilies?.[0] ??
    documentFont ??
    firstBlock?.style?.fontFamilies?.[0] ??
    (hasBlocks ? fallbackFontFaces[0]?.postScriptName : '')
  const currentFontFace =
    findFontFace(fontOptions, currentFontCandidate) ?? fallbackFontFace(currentFontCandidate)
  const currentFont = currentFontFace?.postScriptName ?? ''
  const currentFontFamilyName = currentFontFace?.familyName
  const currentEffect = normalizeEffect(selectedBlock?.style?.effect ?? renderEffect)
  const currentStroke = normalizeStroke(selectedBlock?.style?.stroke ?? renderStroke)
  const currentColor = selectedBlock?.style?.color ?? (hasBlocks ? fallbackColor : DEFAULT_COLOR)
  const currentColorHex = colorToHex(currentColor)
  const currentStrokeColorHex = colorToHex(currentStroke.color)
  const currentStrokeWidth = currentStroke.widthPx ?? DEFAULT_STROKE_WIDTH
  const fontLabel = t('render.fontLabel')
  const fontSizeLabel = t('render.fontSizeLabel', { defaultValue: 'Size' })
  const currentFontSize =
    selectedBlock?.style?.fontSize ??
    selectedBlock?.fontPrediction?.font_size_px ??
    selectedBlock?.detectedFontSizePx
  const effectLabel = t('render.effectLabel')
  const strokeLabel = t('render.effectBorder')
  const strokeColorLabel = t('render.strokeColorLabel')
  const strokeWidthLabel = t('render.strokeWidthLabel')
  const alignLabel = t('render.alignLabel')
  const currentTextAlign = resolveEffectiveTextAlign(selectedBlock ?? firstBlock)
  const scopeLabel =
    selectedBlockIndex !== undefined
      ? t('render.fontScopeBlockIndex', {
          index: selectedBlockIndex + 1,
        })
      : t('render.fontScopeGlobal')
  const scopeToneClass =
    selectedBlockIndex !== undefined
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

  const mergeFontFamilies = (nextFont: string, current: string[] | undefined) => {
    const base = (
      current?.length ? current : fallbackFontFaces.map((font) => font.postScriptName)
    ).map((family) => normalizeFontValue(fontOptions, family) ?? family)
    return [nextFont, ...base.filter((family) => family !== nextFont)]
  }

  const updateDocumentDefaultFont = (value: string) => {
    // Remember as app-level default for future documents
    usePreferencesStore.getState().setDefaultFont(value)
    if (!documentId) return
    void updateDocumentStyle(documentId, {
      defaultFont: value,
    }).then(() =>
      queryClient.invalidateQueries({
        queryKey: getGetDocumentQueryKey(documentId),
      }),
    )
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
    <div className='flex w-full min-w-0 flex-col gap-2'>
      {/* Scope indicator */}
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

      {/* Font + Color */}
      <div className='flex flex-col gap-0.5'>
        <div className='flex items-baseline justify-between'>
          <span className='text-[10px] font-medium text-muted-foreground uppercase'>
            {fontLabel}
          </span>
          <span className='text-[10px] font-medium text-muted-foreground uppercase'>
            {t('render.fontColorLabel')}
          </span>
        </div>
        <div className='flex min-w-0 items-center gap-1.5'>
          <div className='min-w-0 flex-1'>
            <FontSelect
              data-testid='render-font-select'
              value={currentFont}
              options={fontOptions}
              disabled={fontOptions.length === 0}
              placeholder={t('render.fontPlaceholder')}
              triggerStyle={
                currentFontFamilyName ? { fontFamily: currentFontFamilyName } : undefined
              }
              onChange={(value) => {
                const nextFamilies = mergeFontFamilies(value, selectedBlock?.style?.fontFamilies)
                // Only persist a block override when the block already has one.
                // Otherwise keep the block inheriting the document default.
                if (selectedBlockHasExplicitFont) {
                  if (applyStyleToSelected({ fontFamilies: nextFamilies })) return
                }
                updateDocumentDefaultFont(value)
              }}
            />
          </div>
          {selectedBlockHasExplicitFont ? (
            <button
              type='button'
              className='text-[9px] text-muted-foreground hover:text-foreground'
              onClick={() => applyStyleToSelected({ fontFamilies: [] })}
              title='Reset to document default'
            >
              ✕
            </button>
          ) : null}
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
            className='size-7'
          />
        </div>
      </div>

      {/* Size / Effect / Align */}
      <div className='grid w-full grid-cols-[minmax(0,1fr)_auto_auto] items-end gap-x-2'>
        <span className='text-[10px] font-medium text-muted-foreground uppercase'>
          {fontSizeLabel}
        </span>
        <span className='text-[10px] font-medium text-muted-foreground uppercase'>
          {effectLabel}
        </span>
        <span className='text-[10px] font-medium text-muted-foreground uppercase'>
          {alignLabel}
        </span>

        <div className='flex min-w-0 items-center rounded-md border border-input bg-background shadow-xs'>
          <Button
            type='button'
            variant='ghost'
            size='icon-sm'
            aria-label={`${fontSizeLabel} -`}
            className='size-7 shrink-0 rounded-r-none border-r'
            disabled={selectedBlockIndex === undefined}
            onClick={() => {
              const next = Math.max(6, Math.round((currentFontSize ?? 16) - 1))
              applyStyleToSelected({ fontSize: next })
            }}
          >
            <MinusIcon className='size-3' />
          </Button>
          <Input
            type='number'
            step='1'
            min='6'
            max='300'
            inputMode='numeric'
            className='h-7 min-w-0 flex-1 [appearance:textfield] rounded-none border-0 px-1 text-center text-xs shadow-none focus-visible:ring-0 [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none'
            data-testid='render-font-size'
            disabled={selectedBlockIndex === undefined}
            value={currentFontSize !== undefined ? Math.round(currentFontSize) : ''}
            placeholder='auto'
            onChange={(event) => {
              const parsed = Number.parseInt(event.target.value, 10)
              if (!Number.isFinite(parsed) || parsed < 1) return
              applyStyleToSelected({ fontSize: Math.min(300, parsed) })
            }}
          />
          <Button
            type='button'
            variant='ghost'
            size='icon-sm'
            aria-label={`${fontSizeLabel} +`}
            className='size-7 shrink-0 rounded-l-none border-l'
            disabled={selectedBlockIndex === undefined}
            onClick={() => {
              const next = Math.min(300, Math.round((currentFontSize ?? 16) + 1))
              applyStyleToSelected({ fontSize: next })
            }}
          >
            <PlusIcon className='size-3' />
          </Button>
        </div>

        <div className='flex items-center gap-1'>
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
                      'size-7 shrink-0',
                      active &&
                        'border-primary bg-primary text-primary-foreground hover:bg-primary/90',
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

        <div className='flex items-center gap-1'>
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
                      'size-7 shrink-0',
                      active &&
                        'border-primary bg-primary text-primary-foreground hover:bg-primary/90',
                    )}
                    onClick={() => {
                      if (applyStyleToSelected({ textAlign: item.value })) return
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

      {/* Border / Stroke */}
      <div className='flex flex-col gap-0.5'>
        <span className='text-[10px] font-medium text-muted-foreground uppercase'>
          {strokeLabel}
        </span>
        <div className='flex min-w-0 items-center gap-1'>
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
                    'border-primary bg-primary text-primary-foreground hover:bg-primary/90',
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
                  className='size-7'
                />
              </div>
            </TooltipTrigger>
            <TooltipContent side='bottom' sideOffset={4}>
              {strokeColorLabel}
            </TooltipContent>
          </Tooltip>

          <div className='flex min-w-0 flex-1 items-center rounded-md border border-input bg-background shadow-xs'>
            <Button
              type='button'
              variant='ghost'
              size='icon-sm'
              aria-label={`${strokeWidthLabel} -`}
              className='size-7 shrink-0 rounded-r-none border-r'
              onClick={() => updateStrokeWidth(currentStrokeWidth - STROKE_WIDTH_STEP)}
            >
              <MinusIcon className='size-3' />
            </Button>

            <Input
              type='number'
              step={String(STROKE_WIDTH_STEP)}
              min={String(MIN_STROKE_WIDTH)}
              max={String(MAX_STROKE_WIDTH)}
              inputMode='decimal'
              className='h-7 min-w-0 flex-1 [appearance:textfield] rounded-none border-0 px-1 text-center text-xs shadow-none focus-visible:ring-0 [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none'
              data-testid='render-stroke-width'
              value={
                Number.isFinite(currentStrokeWidth) ? currentStrokeWidth : DEFAULT_STROKE_WIDTH
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
              className='size-7 shrink-0 rounded-l-none border-l'
              onClick={() => updateStrokeWidth(currentStrokeWidth + STROKE_WIDTH_STEP)}
            >
              <PlusIcon className='size-3' />
            </Button>
          </div>
        </div>
      </div>
    </div>
  )
}
