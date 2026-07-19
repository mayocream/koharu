'use client'

import {
  AlignCenterIcon,
  AlignLeftIcon,
  AlignRightIcon,
  BoldIcon,
  ItalicIcon,
  MinusIcon,
  PlusIcon,
  SquareDashedIcon,
  SquareIcon,
} from 'lucide-react'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { ColorPicker } from '@/components/ui/color-picker'
import { FontSelect } from '@/components/ui/font-select'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import {
  isTextElement,
  koharuClient,
  useEditorStore,
  type FontFaceView,
  type TextEffect,
  type TextLayout,
  type TextStyle,
} from '@/lib/koharu'
import { cn } from '@/lib/utils'

const DEFAULT_STROKE_COLOR: TextStyle['color'] = [255, 255, 255, 255]
const DEFAULT_STROKE_WIDTH = 1.6
const MIN_STROKE_WIDTH = 0.2
const MAX_STROKE_WIDTH = 24
const STROKE_WIDTH_STEP = 0.1
const DEFAULT_FONT: FontFaceView = {
  family_name: 'Arial',
  post_script_name: 'Arial',
  weight: 400,
  stretch: 100,
  style: 'normal',
  source: 'system',
  category: null,
  cached: true,
}
const FONT_WEIGHT_KEYS: Record<number, string> = {
  100: 'render.fontWeights.thin',
  200: 'render.fontWeights.extraLight',
  300: 'render.fontWeights.light',
  400: 'render.fontWeights.regular',
  500: 'render.fontWeights.medium',
  600: 'render.fontWeights.semiBold',
  700: 'render.fontWeights.bold',
  800: 'render.fontWeights.extraBold',
  900: 'render.fontWeights.black',
}

function clampByte(value: number) {
  return Math.max(0, Math.min(255, Math.round(value)))
}

function clampStrokeWidth(value: number) {
  return Number(Math.max(MIN_STROKE_WIDTH, Math.min(MAX_STROKE_WIDTH, value)).toFixed(1))
}

function colorToHex(color: TextStyle['color']) {
  return `#${color
    .slice(0, 3)
    .map((value) => clampByte(value).toString(16).padStart(2, '0'))
    .join('')}`
}

function hexToColor(value: string, alpha: number): TextStyle['color'] {
  const normalized = value.replace('#', '')
  if (normalized.length !== 6) return [0, 0, 0, clampByte(alpha)]
  const channels = [
    Number.parseInt(normalized.slice(0, 2), 16),
    Number.parseInt(normalized.slice(2, 4), 16),
    Number.parseInt(normalized.slice(4, 6), 16),
  ]
  if (channels.some(Number.isNaN)) return [0, 0, 0, clampByte(alpha)]
  return [channels[0], channels[1], channels[2], clampByte(alpha)]
}

function isStroke(effect: TextEffect) {
  return 'Stroke' in effect.kind
}

function strokeOf(style: TextStyle) {
  const effect = style.effects.find(isStroke)
  const stroke = effect?.kind.Stroke
  if (effect && stroke) {
    return {
      enabled: effect.enabled,
      color: stroke.color,
      width: stroke.width,
    }
  }
  return { enabled: false, color: DEFAULT_STROKE_COLOR, width: DEFAULT_STROKE_WIDTH }
}

function withStroke(
  style: TextStyle,
  stroke: { enabled: boolean; color: TextStyle['color']; width: number },
): TextStyle {
  const currentIndex = style.effects.findIndex(isStroke)
  const current = currentIndex >= 0 ? style.effects[currentIndex] : undefined
  const next: TextEffect = {
    enabled: stroke.enabled,
    opacity: current?.opacity ?? 1,
    blend_mode: current?.blend_mode ?? 'Normal',
    kind: {
      Stroke: {
        color: stroke.color,
        width: clampStrokeWidth(stroke.width),
        position: current?.kind.Stroke?.position ?? 'Center',
      },
    },
  }
  const effects = [...style.effects]
  if (currentIndex >= 0) effects[currentIndex] = next
  else effects.push(next)
  return { ...style, effects }
}

function fontFaceSlant(face: FontFaceView): TextStyle['font_slant'] {
  if (face.style === 'italic') return 'Italic'
  if (face.style === 'oblique') return { Oblique: { angle_degrees: 14 } }
  return 'Normal'
}

function slantName(slant: TextStyle['font_slant']) {
  if (slant === 'Normal') return 'normal'
  if (slant === 'Italic') return 'italic'
  return 'oblique'
}

function variantScore(face: FontFaceView, style: TextStyle | undefined) {
  const weight = style?.font_weight ?? 400
  const stretch = style?.font_stretch ?? 100
  const slant = slantName(style?.font_slant ?? 'Normal')
  return (
    Math.abs(face.weight - weight) +
    Math.abs(face.stretch - stretch) * 2 +
    (face.style === slant ? 0 : 1000)
  )
}

function regularScore(face: FontFaceView) {
  return (
    Math.abs(face.weight - 400) +
    Math.abs(face.stretch - 100) * 2 +
    (face.style === 'normal' ? 0 : 1000)
  )
}

export function RenderControlsPanel() {
  const { t } = useTranslation()
  const page = useEditorStore((state) => state.page)
  const availableFonts = useEditorStore((state) => state.settings?.fonts ?? [])
  const selectedIds = useEditorStore((state) => state.selectedElements)
  const textElements = useMemo(() => page?.elements.filter(isTextElement) ?? [], [page])
  const selected = textElements.filter((element) => selectedIds.includes(element.id))
  const targets = selected.length ? selected : textElements
  const current = selected[0] ?? textElements[0]
  const style = current?.kind.Text.style
  const layout = current?.kind.Text.layout
  const hasText = textElements.length > 0
  const hasSelection = selected.length > 0
  const fontCandidates = useMemo(() => {
    const candidates = [...availableFonts]
    for (const element of textElements) {
      const elementStyle = element.kind.Text.style
      const value = elementStyle.font_families[0]?.trim()
      if (!value) continue
      if (candidates.some((font) => font.family_name === value || font.post_script_name === value))
        continue
      candidates.push({
        family_name: value.split(':')[0],
        post_script_name: value,
        weight: elementStyle.font_weight,
        stretch: Math.round(elementStyle.font_stretch),
        style: slantName(elementStyle.font_slant),
        source: value.includes(':') ? 'google' : 'system',
        category: null,
        cached: true,
      })
    }
    if (!candidates.some((font) => font.family_name === DEFAULT_FONT.family_name))
      candidates.push(DEFAULT_FONT)
    const unique = new Map<string, FontFaceView>()
    for (const font of candidates) unique.set(`${font.source}:${font.post_script_name}`, font)
    return [...unique.values()]
  }, [availableFonts, textElements])
  const familyOptions = useMemo(() => {
    const families = new Map<string, FontFaceView>()
    for (const font of fontCandidates) {
      const existing = families.get(font.family_name)
      if (!existing || (existing.source === 'google' && font.source === 'system'))
        families.set(font.family_name, font)
    }
    return [...families.values()].sort((left, right) =>
      left.family_name.localeCompare(right.family_name),
    )
  }, [fontCandidates])
  const currentFontValue = style?.font_families[0] ?? DEFAULT_FONT.family_name
  const currentFamilyOption =
    fontCandidates.find(
      (font) => font.family_name === currentFontValue || font.post_script_name === currentFontValue,
    ) ?? DEFAULT_FONT
  const currentFamily = currentFamilyOption.family_name
  const currentVariants = useMemo(() => {
    const familyFaces = fontCandidates.filter((font) => font.family_name === currentFamily)
    const source = familyFaces.some((font) => font.source === currentFamilyOption.source)
      ? currentFamilyOption.source
      : familyFaces[0]?.source
    const variants = familyFaces.filter((font) => font.source === source)
    const unique = new Map<string, FontFaceView>()
    for (const font of variants) {
      const key = `${font.weight}:${font.stretch}:${font.style}`
      if (!unique.has(key)) unique.set(key, font)
    }
    return [...unique.values()].sort(
      (left, right) =>
        left.weight - right.weight ||
        left.stretch - right.stretch ||
        left.style.localeCompare(right.style),
    )
  }, [currentFamily, currentFamilyOption.source, fontCandidates])
  const currentVariant = [...currentVariants].sort(
    (left, right) => variantScore(left, style) - variantScore(right, style),
  )[0]
  const currentColor = style?.color ?? [0, 0, 0, 255]
  const currentStroke = style
    ? strokeOf(style)
    : { enabled: false, color: DEFAULT_STROKE_COLOR, width: DEFAULT_STROKE_WIDTH }
  const italic = style?.font_slant !== 'Normal'
  const bold = (style?.font_weight ?? 400) >= 600
  const scopeLabel =
    selected.length > 1
      ? t('render.fontScopeBlocksCount', { count: selected.length })
      : current && hasSelection
        ? t('render.fontScopeBlockIndex', {
            index: textElements.findIndex((element) => element.id === current.id) + 1,
          })
        : t('render.fontScopeGlobal')

  const applyStyles = (mutate: (style: TextStyle) => TextStyle) => {
    if (!page || !targets.length) return
    koharuClient.fire({
      type: 'set_text_styles',
      page: page.id,
      elements: targets.map((element) => ({
        element: element.id,
        style: mutate(element.kind.Text.style),
      })),
    })
  }
  const applyLayouts = (mutate: (layout: TextLayout) => TextLayout) => {
    if (!page || !targets.length) return
    koharuClient.fire({
      type: 'set_text_layouts',
      page: page.id,
      elements: targets.map((element) => ({
        element: element.id,
        layout: mutate(element.kind.Text.layout),
      })),
    })
  }
  const applyStroke = (next: typeof currentStroke) => {
    applyStyles((value) => withStroke(value, next))
  }
  const applyFontFace = (face: FontFaceView) => {
    if (face.source === 'google' && !face.cached) {
      koharuClient.fire({
        type: 'cache_font',
        family: face.family_name,
        weight: face.weight,
        italic: face.style === 'italic',
      })
    }
    const family = face.source === 'google' ? face.post_script_name : face.family_name
    applyStyles((value) => ({
      ...value,
      font_families: [
        family,
        ...value.font_families.filter(
          (item) => item !== family && item !== face.family_name && item !== face.post_script_name,
        ),
      ],
      font_weight: face.weight,
      font_stretch: face.stretch,
      font_slant: fontFaceSlant(face),
    }))
  }
  const variantLabel = (face: FontFaceView) => {
    const weight = FONT_WEIGHT_KEYS[face.weight]
      ? t(FONT_WEIGHT_KEYS[face.weight])
      : String(face.weight)
    const stretched = face.stretch === 100 ? weight : `${weight} · ${face.stretch}%`
    if (face.style === 'italic') return t('render.fontStyles.italicWithName', { name: stretched })
    if (face.style === 'oblique') return `${stretched} Oblique`
    return stretched
  }

  if (!page) {
    return (
      <div className='flex items-center justify-center py-6 text-xs text-muted-foreground'>
        {t('textBlocks.emptyPrompt')}
      </div>
    )
  }

  return (
    <div className='flex w-full min-w-0 flex-col gap-2' data-testid='render-controls-panel'>
      <div className='flex items-center justify-end'>
        <span
          data-testid='render-scope-indicator'
          className={cn(
            'rounded-full border px-2 py-0.5 text-[10px] font-medium tracking-wide uppercase',
            hasSelection
              ? 'border-primary/20 bg-primary/10 text-primary'
              : 'border-border/60 bg-muted text-muted-foreground',
          )}
        >
          {scopeLabel}
        </span>
      </div>

      <div className='flex flex-col gap-0.5'>
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
              value={currentFamily}
              options={familyOptions}
              disabled={!hasText}
              placeholder={t('render.fontPlaceholder')}
              onChange={(family) => {
                const option = familyOptions.find((font) => font.family_name === family)
                if (!option) return
                const variants = fontCandidates
                  .filter((font) => font.family_name === family && font.source === option.source)
                  .sort((left, right) => regularScore(left) - regularScore(right))
                applyFontFace(variants[0] ?? option)
              }}
            />
          </div>
          {currentVariants.length > 1 && currentVariant && (
            <div className='min-w-0 flex-1'>
              <Select
                value={currentVariant.post_script_name}
                onValueChange={(postScriptName) => {
                  const face = currentVariants.find(
                    (variant) => variant.post_script_name === postScriptName,
                  )
                  if (face) applyFontFace(face)
                }}
              >
                <SelectTrigger
                  data-testid='render-font-variant-select'
                  className='h-7 w-full min-w-0 px-2 text-xs'
                >
                  <SelectValue placeholder={t('render.fontStylePlaceholder')} />
                </SelectTrigger>
                <SelectContent>
                  {currentVariants.map((variant) => (
                    <SelectItem key={variant.post_script_name} value={variant.post_script_name}>
                      {variantLabel(variant)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          )}
          <ColorPicker
            value={colorToHex(currentColor)}
            disabled={!hasText}
            triggerTestId='render-color-trigger'
            pickerTestId='render-color-picker'
            swatchTestId='render-color-swatch'
            inputTestId='render-color-input'
            onChange={(hex) =>
              applyStyles((value) => ({
                ...value,
                color: hexToColor(hex, value.color[3]),
              }))
            }
            className='size-7'
          />
        </div>
      </div>

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
            disabled={!hasSelection}
            onClick={() =>
              applyStyles((value) => ({ ...value, font_size: Math.max(6, value.font_size - 1) }))
            }
          >
            <MinusIcon className='size-3' />
          </Button>
          <Input
            type='number'
            min='6'
            max='300'
            className='h-6 min-w-0 flex-1 [appearance:textfield] rounded-none border-0 px-0.5 text-center text-xs shadow-none focus-visible:ring-0 [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none'
            data-testid='render-font-size'
            disabled={!hasSelection}
            value={style ? Math.round(style.font_size) : ''}
            onChange={(event) => {
              const value = Number.parseFloat(event.currentTarget.value)
              if (Number.isFinite(value) && value >= 1)
                applyStyles((style) => ({ ...style, font_size: Math.min(300, value) }))
            }}
          />
          <Button
            type='button'
            variant='ghost'
            size='icon-sm'
            className='size-6 shrink-0 rounded-l-none border-l'
            disabled={!hasSelection}
            onClick={() =>
              applyStyles((value) => ({ ...value, font_size: Math.min(300, value.font_size + 1) }))
            }
          >
            <PlusIcon className='size-3' />
          </Button>
        </div>

        <div className='flex items-center gap-0.5'>
          {[
            { key: 'italic', active: italic, label: t('render.effectItalic'), Icon: ItalicIcon },
            { key: 'bold', active: bold, label: t('render.effectBold'), Icon: BoldIcon },
          ].map(({ key, active, label, Icon }) => (
            <Tooltip key={key}>
              <TooltipTrigger asChild>
                <Button
                  variant={active ? 'toggle_on' : 'toggle_off'}
                  size='icon-sm'
                  aria-label={label}
                  data-testid={`render-effect-toggle-${key}`}
                  disabled={!hasText}
                  className='size-6 shrink-0'
                  onClick={() =>
                    applyStyles((value) =>
                      key === 'bold'
                        ? { ...value, font_weight: active ? 400 : 700 }
                        : { ...value, font_slant: active ? 'Normal' : 'Italic' },
                    )
                  }
                >
                  <Icon className='size-3' />
                </Button>
              </TooltipTrigger>
              <TooltipContent side='bottom' sideOffset={4}>
                {label}
              </TooltipContent>
            </Tooltip>
          ))}
        </div>

        <div className='flex items-center gap-0.5'>
          {[
            { value: 'Start' as const, label: t('render.alignLeft'), Icon: AlignLeftIcon },
            { value: 'Center' as const, label: t('render.alignCenter'), Icon: AlignCenterIcon },
            { value: 'End' as const, label: t('render.alignRight'), Icon: AlignRightIcon },
          ].map(({ value, label, Icon }) => (
            <Tooltip key={value}>
              <TooltipTrigger asChild>
                <Button
                  variant={layout?.horizontal_align === value ? 'toggle_on' : 'toggle_off'}
                  size='icon-sm'
                  aria-label={label}
                  data-testid={`render-align-${value.toLowerCase()}`}
                  disabled={!hasText}
                  className='size-6 shrink-0'
                  onClick={() =>
                    applyLayouts((current) => ({ ...current, horizontal_align: value }))
                  }
                >
                  <Icon className='size-3' />
                </Button>
              </TooltipTrigger>
              <TooltipContent side='bottom' sideOffset={4}>
                {label}
              </TooltipContent>
            </Tooltip>
          ))}
        </div>
      </div>

      <div className='flex flex-col gap-0.5'>
        <span className='text-[10px] font-medium text-muted-foreground uppercase'>
          {t('render.effectBorder')}
        </span>
        <div className='flex min-w-0 items-center gap-1'>
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant={currentStroke.enabled ? 'toggle_on' : 'toggle_off'}
                size='icon-sm'
                aria-label={t('render.effectBorder')}
                data-testid='render-stroke-enable'
                disabled={!hasText}
                className='size-7 shrink-0'
                onClick={() => applyStroke({ ...currentStroke, enabled: !currentStroke.enabled })}
              >
                {currentStroke.enabled ? (
                  <SquareIcon className='size-3.5' strokeWidth={3} />
                ) : (
                  <SquareDashedIcon className='size-3.5' />
                )}
              </Button>
            </TooltipTrigger>
            <TooltipContent side='bottom' sideOffset={4}>
              {t('render.effectBorder')}
            </TooltipContent>
          </Tooltip>

          <ColorPicker
            value={colorToHex(currentStroke.color)}
            disabled={!hasText}
            triggerTestId='render-stroke-color-trigger'
            pickerTestId='render-stroke-color-picker'
            swatchTestId='render-stroke-color-swatch'
            inputTestId='render-stroke-color-input'
            onChange={(hex) =>
              applyStroke({
                ...currentStroke,
                color: hexToColor(hex, currentStroke.color[3]),
              })
            }
            className='size-7'
          />

          <div className='flex min-w-0 flex-1 items-center rounded-md border border-input bg-background shadow-xs'>
            <Button
              type='button'
              variant='ghost'
              size='icon-sm'
              aria-label={`${t('render.strokeWidthLabel')} -`}
              className='size-7 shrink-0 rounded-r-none border-r'
              disabled={!hasText}
              onClick={() =>
                applyStroke({
                  ...currentStroke,
                  width: clampStrokeWidth(currentStroke.width - STROKE_WIDTH_STEP),
                })
              }
            >
              <MinusIcon className='size-3' />
            </Button>
            <Input
              type='number'
              step={String(STROKE_WIDTH_STEP)}
              min={String(MIN_STROKE_WIDTH)}
              max={String(MAX_STROKE_WIDTH)}
              className='h-7 min-w-0 flex-1 [appearance:textfield] rounded-none border-0 px-1 text-center text-xs shadow-none focus-visible:ring-0 [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none'
              data-testid='render-stroke-width'
              disabled={!hasText}
              value={currentStroke.width}
              onChange={(event) => {
                const value = Number.parseFloat(event.currentTarget.value)
                if (Number.isFinite(value)) applyStroke({ ...currentStroke, width: value })
              }}
            />
            <Button
              type='button'
              variant='ghost'
              size='icon-sm'
              aria-label={`${t('render.strokeWidthLabel')} +`}
              className='size-7 shrink-0 rounded-l-none border-l'
              disabled={!hasText}
              onClick={() =>
                applyStroke({
                  ...currentStroke,
                  width: clampStrokeWidth(currentStroke.width + STROKE_WIDTH_STEP),
                })
              }
            >
              <PlusIcon className='size-3' />
            </Button>
          </div>
        </div>
      </div>
    </div>
  )
}
