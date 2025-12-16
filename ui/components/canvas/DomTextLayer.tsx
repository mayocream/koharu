'use client'

import { forwardRef, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { detect } from 'tinyld'
import { TextBlock } from '@/types'

type DomTextLayerProps = {
  blocks: TextBlock[]
  scale: number
  visible: boolean
  image: number[]
}

type RgbOrRgba = [number, number, number] | [number, number, number, number]
type FontInfo = {
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

const isColorTuple = (value: unknown): value is RgbOrRgba =>
  Array.isArray(value) &&
  (value.length === 3 || value.length === 4) &&
  value.every((channel) => typeof channel === 'number')

const pickColor = (...candidates: unknown[]): RgbOrRgba | undefined =>
  candidates.find(isColorTuple)

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
  const list = families?.length ? families : fallback
  return list
    .map((name) => (name.includes(' ') ? `"${name}"` : name))
    .join(', ')
}

const shouldUseVertical = (block: TextBlock, text: string) =>
  block.width < block.height && !/[A-Za-z0-9]/.test(text)

export const DomTextLayer = forwardRef<HTMLDivElement, DomTextLayerProps>(
  function DomTextLayer({ blocks, scale, visible }, ref) {
    const renderBlocks = blocks ?? []
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
          const key = `${block?.x}-${block?.y}-${block?.width}-${block?.height}`
          return (
            <DomTextBlock
              key={`${block?.x}-${block?.y}-${index}`}
              block={block}
              scale={scale}
            />
          )
        })}
      </div>
    )
  },
)

function DomTextBlock({
  block,
  scale,
  serifPreferred,
}: {
  block: TextBlock
  scale: number
  serifPreferred?: boolean
}) {
  const text = block.translation ?? block.text
  if (!text) return null

  const style = block.style as BlockStyle
  const fontInfo =
    (block as { fontInfo?: FontInfo; font_info?: FontInfo }).fontInfo ??
    (block as { fontInfo?: FontInfo; font_info?: FontInfo }).font_info

  const textRef = useRef<HTMLDivElement>(null)
  const requestedSize = block.style?.fontSize
  const lineHeightRatio = block.style?.lineHeight ?? 1.2
  const writingMode = useMemo(
    () => (shouldUseVertical(block, text) ? 'vertical-rl' : 'horizontal-tb'),
    [block.height, block.width, text],
  )
  const fontFamily = useMemo(
    () => formatFontFamilies(block.style?.fontFamilies, text, serifPreferred),
    [block.style?.fontFamilies, serifPreferred, text],
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
    pickColor(fontInfo?.text_color, fontInfo?.textColor, style?.color),
  )
  const strokeColor = pickColor(
    fontInfo?.stroke_color,
    fontInfo?.strokeColor,
    style?.strokeColor,
  )
  const baseStrokeWidth =
    pickNumber(
      fontInfo?.stroke_width_px,
      fontInfo?.strokeWidthPx,
      style?.strokeWidth,
    ) ?? 0
  const referenceFontSize = pickNumber(
    fontInfo?.font_size_px,
    fontInfo?.fontSizePx,
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
      setFontSize(best)
    } else {
      applySize(fontSize)
    }
  }, [
    fontSize,
    initialSize,
    lineHeightRatio,
    requestedSize,
    text,
    block.width,
    block.height,
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
        overflow: 'wrap',
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
          alignItems: 'center',
          justifyContent: 'center',
          textAlign: 'center',
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
