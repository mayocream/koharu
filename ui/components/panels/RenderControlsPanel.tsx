'use client'

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
import { type ComponentType, useMemo, useRef, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { ColorPicker } from '@/components/ui/color-picker'
import { FontSelect, useGoogleFontPreview } from '@/components/ui/font-select'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { VariantItem } from '@/components/ui/variant-item'
import {
  useCurrentPage,
  useSelectedTextNode,
  useSelectedTextNodes,
  useTextNodes,
  type TextNodeEntry,
} from '@/hooks/useCurrentPage'
import { fetchGoogleFont, useGetGoogleFontsCatalog, useListFonts } from '@/lib/api/default/default'
import type {
  FontFaceInfo,
  FontPrediction,
  Op,
  TextAlign,
  TextShaderEffect,
  TextStrokeStyle,
  TextStyle,
} from '@/lib/api/schemas'
import {
  findFontFace,
  getLocalizedFontLabel,
  normalizeFamilyName,
  STYLE_KEYWORDS,
  uniqueFontFaces,
} from '@/lib/font-utils'
import { applyOp, invalidateScene, queueAutoRender } from '@/lib/io/scene'
import { ops } from '@/lib/ops'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'
import { cn } from '@/lib/utils'

const DEFAULT_COLOR: number[] = [0, 0, 0, 255]
const DEFAULT_STROKE_COLOR: number[] = [255, 255, 255, 255]
const DEFAULT_STROKE_WIDTH = 1.6
const MIN_STROKE_WIDTH = 0.2
const MAX_STROKE_WIDTH = 24
const STROKE_WIDTH_STEP = 0.1

const DEFAULT_FONT_FACES: FontFaceInfo[] = [
  {
    familyName: 'Arial',
    postScriptName: 'ArialMT',
    source: 'system',
    cached: true,
  },
]

const clampByte = (v: number) => Math.max(0, Math.min(255, Math.round(v)))
const clampStrokeWidth = (v: number) =>
  Number(Math.max(MIN_STROKE_WIDTH, Math.min(MAX_STROKE_WIDTH, v)).toFixed(1))

const colorToHex = (color: number[]) =>
  `#${color
    .slice(0, 3)
    .map((v) => clampByte(v).toString(16).padStart(2, '0'))
    .join('')}`

const hexToColor = (value: string, alpha: number): number[] => {
  const normalized = value.replace('#', '')
  if (normalized.length !== 6) return [0, 0, 0, clampByte(alpha)]
  const r = Number.parseInt(normalized.slice(0, 2), 16)
  const g = Number.parseInt(normalized.slice(2, 4), 16)
  const b = Number.parseInt(normalized.slice(4, 6), 16)
  if ([r, g, b].some((c) => Number.isNaN(c))) return [0, 0, 0, clampByte(alpha)]
  return [r, g, b, clampByte(alpha)]
}

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

const normalizeStroke = (stroke?: TextStrokeStyle | null): TextStrokeStyle => ({
  enabled: stroke?.enabled ?? true,
  color: stroke?.color ?? DEFAULT_STROKE_COLOR,
  widthPx: stroke?.widthPx ?? null,
})

const normalizeEffect = (effect?: TextShaderEffect | null): TextShaderEffect => ({
  bold: effect?.bold ?? false,
  italic: effect?.italic ?? false,
})

const predictionColor = (prediction?: FontPrediction | null): number[] | undefined => {
  const tc = prediction?.textColor
  if (!tc || tc.length < 3) return undefined
  return [clampByte(tc[0]), clampByte(tc[1]), clampByte(tc[2]), 255]
}

// Mirrors renderer precedence: explicit style color → predicted color → black.
const effectiveColorOf = (style?: TextStyle | null, prediction?: FontPrediction | null): number[] =>
  style?.color ?? predictionColor(prediction) ?? DEFAULT_COLOR

const hasExplicitColor = (node: TextNodeEntry) => Array.isArray(node.data.style?.color)

export function RenderControlsPanel() {
  const { t } = useTranslation()
  const page = useCurrentPage()
  const textNodes = useTextNodes()
  const selectedNode = useSelectedTextNode()
  const selectedNodes = useSelectedTextNodes()
  const { data: availableFonts = [] } = useListFonts()
  useGetGoogleFontsCatalog() // prefetch catalog so picker can decorate Google entries
  const appDefaultFont = usePreferencesStore((s) => s.defaultFont)
  const favoriteFonts = usePreferencesStore((s) => s.favoriteFonts)
  const toggleFavoriteFont = usePreferencesStore((s) => s.toggleFavoriteFont)
  const renderEffect = useEditorUiStore((s) => s.renderEffect)
  const setRenderEffect = useEditorUiStore((s) => s.setRenderEffect)
  const setRenderStroke = useEditorUiStore((s) => s.setRenderStroke)

  const sortedFonts = useMemo(() => {
    return [...(availableFonts ?? [])].sort((a, b) => a.familyName.localeCompare(b.familyName))
  }, [availableFonts])

  const sectionRef = useRef<HTMLDivElement>(null)
  const [sectionWidth, setSectionWidth] = useState<number>(0)

  useEffect(() => {
    if (!sectionRef.current) return
    const observer = new ResizeObserver((entries) => {
      setSectionWidth(entries[0].contentRect.width)
    })
    observer.observe(sectionRef.current)
    return () => observer.disconnect()
  }, [])

  const firstNode = textNodes[0]
  const hasNodes = textNodes.length > 0

  const fontCandidates = useMemo(
    () =>
      uniqueFontFaces(
        [
          ...sortedFonts,
          ...(appDefaultFont ? [fallbackFontFace(appDefaultFont)] : []),
          ...(selectedNode?.data.style?.fontFamilies?.slice(0, 1)?.map(fallbackFontFace) ?? []),
          ...(firstNode?.data.style?.fontFamilies?.slice(0, 1)?.map(fallbackFontFace) ?? []),
          ...DEFAULT_FONT_FACES,
        ].filter((v): v is FontFaceInfo => !!v),
      ),
    [sortedFonts, appDefaultFont, selectedNode?.id, selectedNode?.data.style?.fontFamilies],
  )

  const currentFontCandidate =
    selectedNode?.data.style?.fontFamilies?.[0] ??
    appDefaultFont ??
    firstNode?.data.style?.fontFamilies?.[0] ??
    (hasNodes ? fontCandidates[0]?.postScriptName : '')
  const currentFontFace = useMemo(() => {
    return (
      findFontFace(fontCandidates, currentFontCandidate) || fallbackFontFace(currentFontCandidate)
    )
  }, [fontCandidates, currentFontCandidate])

  const currentFont = currentFontFace?.postScriptName ?? ''
  const currentFontFamilyName = useMemo(() => {
    if (!currentFontFace) return undefined
    return normalizeFamilyName(currentFontFace.familyName)
  }, [currentFontFace])

  const familyOptions = useMemo(() => {
    const families = new Map<string, FontFaceInfo>()
    for (const f of fontCandidates) {
      const name = normalizeFamilyName(f.familyName)
      if (!families.has(name) || f.postScriptName === name) {
        families.set(name, { ...f, familyName: name }) // Use normalized name for the option
      }
    }
    return Array.from(families.values()).sort((a, b) => a.familyName.localeCompare(b.familyName))
  }, [fontCandidates])

  const currentVariants = useMemo(() => {
    const name = normalizeFamilyName(currentFontFamilyName ?? '').toLowerCase()
    if (!name) return []
    const nameNoSpace = name.replace(/\s+/g, '')
    return fontCandidates.filter((f) => {
      const fFamilyNorm = normalizeFamilyName(f.familyName).toLowerCase()
      if (fFamilyNorm === name) return true

      const fPsNorm = f.postScriptName.toLowerCase()
      if (fPsNorm.includes(nameNoSpace)) {
        // Ensure the family part of the PS name is an EXACT match
        const familyPart = f.postScriptName
          .split(/[:\-_]/)[0]
          .replace(/[\s\-_]+/g, '')
          .toLowerCase()
        if (familyPart !== nameNoSpace) return false

        const rest = fPsNorm.replace(nameNoSpace, '')
        const isStyleSuffix =
          !rest ||
          /^[-_\s]/.test(rest) ||
          STYLE_KEYWORDS.some((k) => rest.toLowerCase().includes(k.toLowerCase()))

        if (isStyleSuffix) return true
      }
      return false
    })
  }, [fontCandidates, currentFontFamilyName])

  const currentVariantsWithLabels = useMemo(() => {
    if (!currentVariants) return []

    // First pass: generate all labels
    const mapped = currentVariants.map((v) => ({
      variant: v,
      label: getLocalizedFontLabel(v, t),
    }))

    // Second pass: identify duplicates
    return mapped.map((item) => {
      const isDuplicate =
        mapped.filter(
          (other) =>
            other.variant.postScriptName !== item.variant.postScriptName &&
            other.label === item.label,
        ).length > 0

      return {
        ...item,
        isDuplicate,
      }
    })
  }, [currentVariants, t])

  const selectedStyle = selectedNode?.data.style ?? firstNode?.data.style
  const colorSource = selectedNode ?? firstNode
  const currentColor = effectiveColorOf(colorSource?.data.style, colorSource?.data.fontPrediction)
  const currentColorHex = colorToHex(currentColor)
  const currentStroke = normalizeStroke(selectedStyle?.stroke)
  const currentStrokeColorHex = colorToHex(currentStroke.color ?? DEFAULT_STROKE_COLOR)
  const currentStrokeWidth = currentStroke.widthPx ?? DEFAULT_STROKE_WIDTH
  const currentEffect = normalizeEffect(selectedStyle?.effect ?? renderEffect)
  // The scene only persists manual overrides in `style.fontSize`. Font detector
  // metadata describes the source text, not the renderer's current auto-fit size.
  const currentFontSize: number | undefined = selectedNode?.data.style?.fontSize ?? undefined

  const effectiveAlign: TextAlign =
    selectedNode?.data.style?.textAlign ??
    firstNode?.data.style?.textAlign ??
    (selectedNode?.data.translation ? 'center' : 'left')

  const currentFontPreviewState = useGoogleFontPreview(
    currentFontFace?.source === 'google' ? currentFont : (currentFontFamilyName ?? ''),
    currentFontFace?.source ?? 'system',
    true,
  )

  // ---------------------------------------------------------------------------
  // Mutations
  // ---------------------------------------------------------------------------

  const buildStyleOp = (n: TextNodeEntry, updates: Partial<TextStyle>): Op => {
    const current = n.data.style
    const nextStyle: TextStyle = {
      fontFamilies: updates.fontFamilies ?? current?.fontFamilies ?? [],
      fontSize: updates.fontSize ?? current?.fontSize ?? null,
      color: updates.color ?? effectiveColorOf(current, n.data.fontPrediction),
      effect: updates.effect ?? current?.effect ?? null,
      stroke: updates.stroke ?? current?.stroke ?? null,
      textAlign: updates.textAlign ?? current?.textAlign ?? null,
    }
    return ops.updateNode(page!.id, n.id, {
      data: { text: { style: nextStyle } } as never,
    })
  }

  const applyStyleToNodes = (
    nodes: TextNodeEntry[],
    updates: Partial<TextStyle>,
    label: string,
  ) => {
    if (!page || nodes.length === 0) return
    void (async () => {
      const op =
        nodes.length === 1
          ? buildStyleOp(nodes[0], updates)
          : ops.batch(
              label,
              nodes.map((n) => buildStyleOp(n, updates)),
            )
      await applyOp(op)
      queueAutoRender(page.id)
    })()
  }

  const applyStyleToSelected = (updates: Partial<TextStyle>): boolean => {
    if (selectedNodes.length === 0) return false
    applyStyleToNodes(selectedNodes, updates, 'Multi-block style update')
    return true
  }

  const applyStyleToAll = (updates: Partial<TextStyle>) => {
    applyStyleToNodes(textNodes, updates, 'Bulk style update')
  }

  const commitCurrentFontColorIfImplicit = () => {
    const targets = selectedNodes.length > 0 ? selectedNodes : textNodes
    if (targets.every(hasExplicitColor)) return
    applyStyleToNodes(targets, { color: currentColor }, 'Explicit font color update')
  }

  const applyStrokeSetting = (nextStroke: TextStrokeStyle) => {
    if (applyStyleToSelected({ stroke: normalizeStroke(nextStroke) })) return
    setRenderStroke({
      enabled: nextStroke.enabled ?? true,
      color: (nextStroke.color ?? DEFAULT_STROKE_COLOR) as [number, number, number, number],
      widthPx: nextStroke.widthPx ?? undefined,
    })
  }

  const updateStrokeWidth = (value: number) => {
    applyStrokeSetting({ ...currentStroke, widthPx: clampStrokeWidth(value) })
  }

  const effectItems: {
    key: 'italic' | 'bold'
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
    { value: 'left', label: t('render.alignLeft'), Icon: AlignLeftIcon },
    { value: 'center', label: t('render.alignCenter'), Icon: AlignCenterIcon },
    { value: 'right', label: t('render.alignRight'), Icon: AlignRightIcon },
  ]

  const scopeLabel =
    selectedNodes.length > 1
      ? t('render.fontScopeBlocksCount', { count: selectedNodes.length })
      : selectedNode
        ? t('render.fontScopeBlockIndex', {
            index: textNodes.findIndex((n) => n.id === selectedNode.id) + 1,
          })
        : t('render.fontScopeGlobal')
  const scopeToneClass = selectedNode
    ? 'border-primary/20 bg-primary/10 text-primary'
    : 'border-border/60 bg-muted text-muted-foreground'

  if (!page) {
    return (
      <div className='flex items-center justify-center py-6 text-xs text-muted-foreground'>
        {t('textBlocks.emptyPrompt')}
      </div>
    )
  }

  return (
    <div className='flex w-full min-w-0 flex-col gap-2'>
      {/* Scope */}
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
      <div className='flex flex-col gap-0.5' ref={sectionRef}>
        <div className='flex items-baseline justify-between'>
          <span className='text-[10px] font-medium text-muted-foreground uppercase'>
            {t('render.fontLabel')}
          </span>
          <span className='text-[10px] font-medium text-muted-foreground uppercase'>
            {t('render.fontColorLabel')}
          </span>
        </div>
        <div className='flex min-w-0 items-center gap-1.5'>
          <div className='min-w-0 flex-[1.5]'>
            <FontSelect
              data-testid='render-font-select'
              value={currentFontFamilyName ?? ''}
              options={familyOptions}
              favoriteFonts={favoriteFonts}
              onToggleFavorite={toggleFavoriteFont}
              disabled={familyOptions.length === 0}
              placeholder={t('render.fontPlaceholder')}
              triggerStyle={
                currentFontFamilyName ? { fontFamily: currentFontFamilyName } : undefined
              }
              contentStyle={
                sectionWidth > 0 ? { width: sectionWidth, maxWidth: sectionWidth } : undefined
              }
              onChange={async (value) => {
                const familyVariants = fontCandidates.filter(
                  (f) => normalizeFamilyName(f.familyName) === value,
                )
                // Try to find Regular/400 first
                const regularFace =
                  familyVariants.find((f) => {
                    const ps = f.postScriptName.toLowerCase()
                    return ps.includes('regular') || ps.includes('400') || ps.includes(':400')
                  }) || familyVariants[0]

                const face = regularFace || findFontFace(fontCandidates, value)
                if (!face) return

                // Trigger fetch for Google Fonts if not cached
                if (face.source === 'google' && !face.cached) {
                  try {
                    await fetchGoogleFont(encodeURIComponent(face.postScriptName))
                    invalidateScene()
                  } catch (e) {
                    console.error('Failed to fetch font:', e)
                  }
                }

                if (selectedNode) {
                  applyStyleToSelected({ fontFamilies: [face.postScriptName] })
                  return
                }
                usePreferencesStore.getState().setDefaultFont(face.postScriptName)
              }}
            />
          </div>
          {currentVariants && currentVariants.length > 1 && (
            <div className='min-w-0 flex-1'>
              <Select
                key={`${currentFontFamilyName}-${currentVariants.length}`}
                value={currentFont}
                onValueChange={async (value) => {
                  // Trigger fetch for Google Fonts if not cached
                  const variant = currentVariants.find((v) => v.postScriptName === value)
                  if (variant?.source === 'google' && !variant.cached) {
                    try {
                      await fetchGoogleFont(encodeURIComponent(value))
                      invalidateScene()
                    } catch (e) {
                      console.error('Failed to fetch font variant:', e)
                    }
                  }

                  if (selectedNode) {
                    applyStyleToSelected({ fontFamilies: [value] })
                    return
                  }
                  usePreferencesStore.getState().setDefaultFont(value)
                }}
              >
                <SelectTrigger
                  className='h-7 w-full px-2 text-xs'
                  style={{
                    fontFamily:
                      currentFontPreviewState === 'ready'
                        ? `"${(currentFontFace?.source === 'google' ? currentFont : (currentFontFamilyName ?? '')).replace(':', '-')}"`
                        : undefined,
                  }}
                >
                  <SelectValue placeholder={t('render.fontStylePlaceholder')} />
                </SelectTrigger>
                <SelectContent
                  position='popper'
                  style={
                    sectionWidth > 0 ? { width: sectionWidth, maxWidth: sectionWidth } : undefined
                  }
                  className='overflow-hidden p-0'
                  align='start'
                  sideOffset={4}
                >
                  {currentVariantsWithLabels.map(({ variant, label, isDuplicate }) => (
                    <VariantItem
                      key={variant.postScriptName}
                      variant={variant}
                      label={
                        isDuplicate
                          ? `${label} (${variant.source === 'google' ? 'Google' : 'System'})`
                          : label
                      }
                    />
                  ))}
                </SelectContent>
              </Select>
            </div>
          )}
          <ColorPicker
            value={currentColorHex}
            disabled={!hasNodes}
            triggerTestId='render-color-trigger'
            pickerTestId='render-color-picker'
            swatchTestId='render-color-swatch'
            inputTestId='render-color-input'
            pickButtonTestId='render-color-pick'
            onOpenChange={(open) => {
              if (open) commitCurrentFontColorIfImplicit()
            }}
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
      <div className='grid w-full grid-cols-[minmax(0,1fr)_auto_auto] items-end gap-x-1.5'>
        <span className='text-[10px] font-medium text-muted-foreground uppercase'>
          {t('render.fontSizeLabel')}
        </span>
        <span className='text-[10px] font-medium text-muted-foreground uppercase'>
          {t('render.effectLabel')}
        </span>
        <span className='text-[10px] font-medium text-muted-foreground uppercase'>
          {t('render.alignLabel')}
        </span>

        <div className='flex min-w-0 items-center rounded-md border border-input bg-background shadow-xs'>
          <Button
            type='button'
            variant='ghost'
            size='icon-sm'
            className='size-6 shrink-0 rounded-r-none border-r'
            disabled={!selectedNode}
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
            className='h-6 min-w-0 flex-1 [appearance:textfield] rounded-none border-0 px-0.5 text-center text-xs shadow-none focus-visible:ring-0 [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none'
            data-testid='render-font-size'
            disabled={!selectedNode}
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
            className='size-6 shrink-0 rounded-l-none border-l'
            disabled={!selectedNode}
            onClick={() => {
              const next = Math.min(300, Math.round((currentFontSize ?? 16) + 1))
              applyStyleToSelected({ fontSize: next })
            }}
          >
            <PlusIcon className='size-3' />
          </Button>
        </div>

        <div className='flex items-center gap-0.5'>
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
                      'size-6 shrink-0',
                      active &&
                        'border-primary bg-primary text-primary-foreground hover:bg-primary/90',
                    )}
                    onClick={() => {
                      const nextEffect: TextShaderEffect = {
                        ...currentEffect,
                        [item.key]: !active,
                      }
                      if (applyStyleToSelected({ effect: nextEffect })) return
                      setRenderEffect({
                        bold: nextEffect.bold ?? false,
                        italic: nextEffect.italic ?? false,
                      })
                    }}
                  >
                    <Icon className='size-3' />
                  </Button>
                </TooltipTrigger>
                <TooltipContent side='bottom' sideOffset={4}>
                  {item.label}
                </TooltipContent>
              </Tooltip>
            )
          })}
        </div>

        <div className='flex items-center gap-0.5'>
          {textAlignItems.map((item) => {
            const active = effectiveAlign === item.value
            const Icon = item.Icon
            return (
              <Tooltip key={item.value}>
                <TooltipTrigger asChild>
                  <Button
                    variant='outline'
                    size='icon-sm'
                    aria-label={item.label}
                    data-testid={`render-align-${item.value}`}
                    disabled={!hasNodes}
                    className={cn(
                      'size-6 shrink-0',
                      active &&
                        'border-primary bg-primary text-primary-foreground hover:bg-primary/90',
                    )}
                    onClick={() => {
                      if (applyStyleToSelected({ textAlign: item.value })) return
                      applyStyleToAll({ textAlign: item.value })
                    }}
                  >
                    <Icon className='size-3' />
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
          {t('render.effectBorder')}
        </span>
        <div className='flex min-w-0 items-center gap-1'>
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant='outline'
                size='icon-sm'
                data-testid='render-stroke-enable'
                className={cn(
                  'size-7 shrink-0',
                  currentStroke.enabled &&
                    'border-primary bg-primary text-primary-foreground hover:bg-primary/90',
                )}
                onClick={() =>
                  applyStrokeSetting({ ...currentStroke, enabled: !currentStroke.enabled })
                }
              >
                <SquareIcon className='size-3.5' />
              </Button>
            </TooltipTrigger>
            <TooltipContent side='bottom' sideOffset={4}>
              {t('render.effectBorder')}
            </TooltipContent>
          </Tooltip>

          <Tooltip>
            <TooltipTrigger asChild>
              <div>
                <ColorPicker
                  value={currentStrokeColorHex}
                  disabled={!hasNodes}
                  triggerTestId='render-stroke-color-trigger'
                  pickerTestId='render-stroke-color-picker'
                  swatchTestId='render-stroke-color-swatch'
                  inputTestId='render-stroke-color-input'
                  pickButtonTestId='render-stroke-color-pick'
                  onChange={(hex) => {
                    applyStrokeSetting({
                      ...currentStroke,
                      color: hexToColor(
                        hex,
                        (currentStroke.color ?? DEFAULT_STROKE_COLOR)[3] ?? 255,
                      ),
                    })
                  }}
                  className='size-7'
                />
              </div>
            </TooltipTrigger>
            <TooltipContent side='bottom' sideOffset={4}>
              {t('render.strokeColorLabel')}
            </TooltipContent>
          </Tooltip>

          <div className='flex min-w-0 flex-1 items-center rounded-md border border-input bg-background shadow-xs'>
            <Button
              type='button'
              variant='ghost'
              size='icon-sm'
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
