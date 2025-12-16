'use client'

import { forwardRef, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { detect } from 'tinyld'
import { TextBlock } from '@/types'
import { useTextBlocks } from '@/hooks/useTextBlocks'

type DomTextLayerProps = {
  blocks: TextBlock[]
  scale: number
  visible: boolean
}

type RgbOrRgba = [number, number, number] | [number, number, number, number]
type FontPrediction = {
  text_color?: [number, number, number]
  textColor?: [number, number, number]
  stroke_color?: [number, number, number]
  strokeColor?: [number, number, number]
  stroke_width_px?: number
  strokeWidthPx?: number
  font_size_px?: number
  fontSizePx?: number
}
type BlockStyle =
  | (TextBlock['style'] & {
      strokeColor?: RgbOrRgba
      strokeWidth?: number
    })
  | undefined

const FALLBACK_FONTS = {
  japanese: {
    serif: ['Yu Mincho', 'Hiragino Mincho', 'Noto Serif JP'],
    sans: ['Noto Sans JP', 'Hiragino Sans', 'Yu Gothic'],
  },
  traditionalChinese: {
    serif: ['BiauKai', 'Kaiti TC', 'DFKai-SB'],
    sans: [
      'PingFang TC',
      'Hiragino Sans TC',
      'Microsoft Jhenghei',
      'Source Han Sans TC',
    ],
  },
  simplifiedChinese: {
    serif: ['Kaiti SC', 'STKaiti', 'FangSong'],
    sans: [
      'PingFang SC',
      'Microsoft YaHei',
      'Source Han Sans SC',
      'WenQuanYi Micro Hei',
      'Droid Sans Fallback',
    ],
  },
  latin: {
    serif: ['Times New Roman', 'Georgia', 'Baskerville'],
    sans: ['Arial', 'SF Pro', 'Helvetica', 'Segoe UI'],
  },
}
const MIN_FONT_SIZE = 1
const MAX_FONT_SIZE = 180
const clampFontSize = (value: number) =>
  Math.min(MAX_FONT_SIZE, Math.max(MIN_FONT_SIZE, value))

const NEAR_BLACK_THRESHOLD = 12
const GRAY_NEAR_BLACK_THRESHOLD = 60
const GRAY_TOLERANCE = 10

const clampNearBlack = (color?: RgbOrRgba): RgbOrRgba | undefined => {
  if (!color) return color
  const [r, g, b] = color
  const maxChannel = Math.max(r, g, b)
  const minChannel = Math.min(r, g, b)
  const isGrayish = maxChannel - minChannel <= GRAY_TOLERANCE
  const threshold = isGrayish ? GRAY_NEAR_BLACK_THRESHOLD : NEAR_BLACK_THRESHOLD

  if (r <= threshold && g <= threshold && b <= threshold) {
    return color.length === 4 ? [0, 0, 0, color[3]] : [0, 0, 0]
  }
  return color
}

const quantile = (values: number[], percentile: number) => {
  if (!values.length) return 0
  const clamped = Math.min(Math.max(percentile, 0), 1)
  const pos = clamped * (values.length - 1)
  const lower = Math.floor(pos)
  const upper = Math.ceil(pos)

  if (lower === upper) return values[lower]

  const weight = pos - lower
  return values[lower] * (1 - weight) + values[upper] * weight
}

const filteredAverage = (samples: Array<number | undefined>) => {
  const values = samples.filter(
    (value): value is number =>
      typeof value === 'number' && Number.isFinite(value),
  )
  if (values.length === 0) return undefined

  const sorted = [...values].sort((a, b) => a - b)
  const q1 = quantile(sorted, 0.25)
  const q3 = quantile(sorted, 0.75)
  const iqr = q3 - q1
  const lower = q1 - 1.5 * iqr
  const upper = q3 + 1.5 * iqr

  const filtered = sorted.filter((v) => v >= lower && v <= upper)
  const finalValues = filtered.length > 0 ? filtered : sorted
  const sum = finalValues.reduce((acc, v) => acc + v, 0)
  return sum / finalValues.length
}

const isColorTuple = (value: unknown): value is RgbOrRgba =>
  Array.isArray(value) &&
  (value.length === 3 || value.length === 4) &&
  value.every((channel) => typeof channel === 'number')

const pickColor = (...candidates: unknown[]): RgbOrRgba | undefined => {
  const found = candidates.find(isColorTuple)
  return found ? (found as RgbOrRgba) : undefined
}

const pickNumber = (...candidates: unknown[]): number | undefined =>
  candidates.find((value): value is number => typeof value === 'number')

const toCssColor = (color?: RgbOrRgba, defaultAlpha = 255) => {
  const r = color?.[0] ?? 0
  const g = color?.[1] ?? 0
  const b = color?.[2] ?? 0
  const a =
    (color as [number, number, number, number] | undefined)?.[3] ?? defaultAlpha
  return `rgba(${r}, ${g}, ${b}, ${a / 255})`
}

const detectScriptForFallback = (text: string) => {
  const lang = detect(text) ?? 'en'

  if (lang.startsWith('ja')) return 'japanese' as const
  if (lang === 'zh-tw' || lang === 'zh-hant')
    return 'traditionalChinese' as const
  if (lang.startsWith('zh')) return 'simplifiedChinese' as const
  return 'latin' as const
}

const formatFontFamilies = (
  families: string[] | undefined,
  text: string,
  serifPreferred?: boolean,
) => {
  const script = detectScriptForFallback(text)
  const fallbackGroup = FALLBACK_FONTS[script]
  const fallback = serifPreferred ? fallbackGroup.serif : fallbackGroup.sans
  const list =
    families && families.length > 0 ? [...families, ...fallback] : fallback
  return list
    .map((name) => (name.includes(' ') ? `"${name}"` : name))
    .join(', ')
}

const shouldUseVertical = (block: TextBlock, text: string) =>
  block.width < block.height && !/[A-Za-z0-9]/.test(text)

export const DomTextLayer = forwardRef<HTMLDivElement, DomTextLayerProps>(
  function DomTextLayer({ blocks, scale, visible }, ref) {
    const renderBlocks = blocks ?? []
    const sharedDefaultFontSize = useMemo(
      () => filteredAverage(renderBlocks.map((block) => block.style?.fontSize)),
      [renderBlocks],
    )
    return (
      <div
        ref={ref}
        data-dom-render-layer
        aria-hidden
        style={{
          position: 'absolute',
          inset: 0,
          width: '100%',
          height: '100%',
          pointerEvents: 'none',
          opacity: visible ? 1 : 0,
          backgroundColor: 'transparent',
        }}
      >
        {renderBlocks?.map((block, index) => {
          return (
            <DomTextBlock
              index={index}
              key={`${block?.x}-${block?.y}-${index}`}
              block={block}
              scale={scale}
              sharedDefaultFontSize={sharedDefaultFontSize}
            />
          )
        })}
      </div>
    )
  },
)

function DomTextBlock({
  index,
  block,
  scale,
  serifPreferred,
  sharedDefaultFontSize,
}: {
  index: number
  block: TextBlock
  scale: number
  serifPreferred?: boolean
  sharedDefaultFontSize?: number
}) {
  const { replaceBlock } = useTextBlocks()
  const text = block.translation ?? block.text
  if (!text) return null

  const style = block.style as BlockStyle
  const fontPrediction =
    (
      block as {
        fontPrediction?: FontPrediction
        font_prediction?: FontPrediction
      }
    ).fontPrediction ??
    (
      block as {
        fontPrediction?: FontPrediction
        font_prediction?: FontPrediction
      }
    ).font_prediction

  const textRef = useRef<HTMLDivElement>(null)
  const requestedSize = style?.fontSize ?? sharedDefaultFontSize
  const lineHeightRatio = 1.2
  const writingMode = useMemo(
    () => (shouldUseVertical(block, text) ? 'vertical-rl' : 'horizontal-tb'),
    [block.height, block.width, text],
  )
  const fontFamily = useMemo(
    () => formatFontFamilies(style?.fontFamilies, text, serifPreferred),
    [style?.fontFamilies, serifPreferred, text],
  )
  const initialSize = useMemo(
    () =>
      clampFontSize(
        requestedSize ??
          Math.max(
            MIN_FONT_SIZE,
            Math.min(Math.max(block.width, block.height) * (scale / 5), 48),
          ),
      ),
    [requestedSize, block.width, block.height, scale],
  )
  const [fontSize, setFontSize] = useState<number>(initialSize)
  const textColor = toCssColor(
    clampNearBlack(
      pickColor(
        fontPrediction?.text_color,
        fontPrediction?.textColor,
        style?.color,
      ),
    ),
  )
  const strokeColor = pickColor(
    fontPrediction?.stroke_color,
    fontPrediction?.strokeColor,
    style?.strokeColor,
  )
  const baseStrokeWidth =
    pickNumber(
      fontPrediction?.stroke_width_px,
      fontPrediction?.strokeWidthPx,
      style?.strokeWidth,
    ) ?? 0
  const referenceFontSize = pickNumber(
    fontPrediction?.font_size_px,
    fontPrediction?.fontSizePx,
    style?.fontSize,
  )
  const strokeWidthPx =
    baseStrokeWidth === 0
      ? 0
      : Math.max(
          0,
          referenceFontSize && referenceFontSize > 0
            ? (baseStrokeWidth * fontSize) / referenceFontSize
            : baseStrokeWidth * scale,
        )
  // disable for now, the detection is not good enough
  // const strokeCss = strokeWidthPx > 0 ? toCssColor(strokeColor) : 'transparent'
  const strokeCss = 'transparent'

  useLayoutEffect(() => {
    const node = textRef.current
    const container = node?.parentElement
    if (!node || !container) return

    const availableWidth = container.clientWidth
    const availableHeight = container.clientHeight
    if (!availableWidth || !availableHeight) return

    const applySize = (size: number) => {
      const clamped = clampFontSize(size)
      node.style.fontSize = `${clamped}px`
      node.style.lineHeight = `${Math.max(
        clamped * lineHeightRatio,
        clamped,
      )}px`
      return clamped
    }

    const fits = (size: number) => {
      const applied = applySize(size)
      return (
        node.scrollWidth <= Math.ceil(availableWidth + 0.5) &&
        node.scrollHeight <= Math.ceil(availableHeight + 0.5) &&
        applied >= MIN_FONT_SIZE
      )
    }

    if (requestedSize && fits(requestedSize)) {
      if (block.style !== undefined && block.style !== null) {
        block.style.fontSize = (requestedSize * block.width) / availableWidth
      }
      replaceBlock(index, block)
      setFontSize(requestedSize)
      return
    }

    let low = MIN_FONT_SIZE
    let high = clampFontSize(
      Math.max(availableWidth, availableHeight, initialSize),
    )
    let best = low

    while (low <= high) {
      const mid = Math.floor((low + high) / 2)
      if (fits(mid)) {
        best = mid
        low = mid + 1
      } else {
        high = mid - 1
      }
    }

    if (best !== fontSize) {
      if (block.style !== undefined && block.style !== null) {
        block.style.fontSize = ((best as number) * block.width) / availableWidth
      }
      replaceBlock(index, block)
      setFontSize(best)
    } else {
      applySize(fontSize)
    }
  }, [
    fontSize,
    initialSize,
    lineHeightRatio,
    requestedSize,
    scale,
    text,
    block.width,
    block.height,
    fontFamily,
    writingMode,
  ])

  return (
    <div
      style={{
        position: 'absolute',
        left: block.x * scale,
        top: block.y * scale,
        width: Math.max(0, block.width * scale),
        height: Math.max(0, block.height * scale),
        pointerEvents: 'none',
        color: textColor,
        background: 'transparent',
      }}
    >
      <div
        ref={textRef}
        style={{
          width: '100%',
          height: '100%',
          display: 'flex',
          //alignItems: 'center',
          //justifyContent: 'center',
          //textAlign: 'center',
          whiteSpace: 'pre-wrap',
          wordBreak: 'normal',
          overflowWrap: 'normal',
          fontFamily,
          fontSize,
          lineHeight: `${Math.max(fontSize * lineHeightRatio, fontSize)}px`,
          writingMode,
          color: textColor,
          backgroundColor: 'transparent',
          WebkitTextStrokeColor: strokeCss,
          WebkitTextStrokeWidth:
            strokeWidthPx > 0 ? `${strokeWidthPx}px` : undefined,
          paintOrder: strokeWidthPx > 0 ? 'stroke fill' : undefined,
          transformOrigin: 'center',
          userSelect: 'none',
        }}
      >
        {text}
      </div>
    </div>
  )
}
