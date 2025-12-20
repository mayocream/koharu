'use client'

import { useEffect, useState } from 'react'
import { TextBlock } from '@/types'
import { convertToBlob } from '@/lib/util'

type TextBlockSpriteLayerProps = {
  blocks?: TextBlock[]
  scale: number
  visible: boolean
}

export function TextBlockSpriteLayer({
  blocks,
  scale,
  visible,
}: TextBlockSpriteLayerProps) {
  const renderBlocks = blocks ?? []

  return (
    <div
      data-text-sprite-layer
      aria-hidden
      style={{
        position: 'absolute',
        inset: 0,
        width: '100%',
        height: '100%',
        pointerEvents: 'none',
        opacity: visible ? 1 : 0,
      }}
    >
      {renderBlocks.map((block, index) => (
        <TextBlockSprite
          key={`${block.x}-${block.y}-${index}`}
          block={block}
          scale={scale}
        />
      ))}
    </div>
  )
}

function TextBlockSprite({
  block,
  scale,
}: {
  block: TextBlock
  scale: number
}) {
  const [src, setSrc] = useState<string | null>(null)
  const sprite = block.rendered

  useEffect(() => {
    if (!sprite?.length) {
      setSrc(null)
      return
    }
    const blob = convertToBlob(sprite)
    const objectUrl = URL.createObjectURL(blob)
    setSrc(objectUrl)
    return () => {
      URL.revokeObjectURL(objectUrl)
    }
  }, [sprite])

  if (!src) return null

  const width = Math.max(0, block.width * scale)
  const height = Math.max(0, block.height * scale)
  if (!width || !height) return null

  return (
    <img
      alt=''
      src={src}
      draggable={false}
      style={{
        position: 'absolute',
        left: block.x * scale,
        top: block.y * scale,
        width,
        height,
        objectFit: 'contain',
        userSelect: 'none',
        pointerEvents: 'none',
      }}
    />
  )
}
