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

const toCssColor = (color?: [number, number, number, number]) => {
  const [r, g, b, a] = color ?? [0, 0, 0, 255]
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
        color: toCssColor(block.style?.color),
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
          backgroundColor: 'transparent',
          transformOrigin: 'center',
          userSelect: 'none',
        }}
      >
        {text}
      </div>
    </div>
  )
}
