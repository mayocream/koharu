'use client'

import type { CSSProperties } from 'react'
import { AutoTextSize } from 'auto-text-size'
import { useAppStore } from '@/lib/store'
import { useTextBlocks } from '@/hooks/useTextBlocks'
import type { TextBlock } from '@/types'

export function TextBlockRenderer() {
  const { textBlocks } = useTextBlocks()
  const scale = useAppStore((state) => state.scale)
  const scaleRatio = scale / 100

  if (!textBlocks.length) {
    return null
  }

  return (
    <div
      className='pointer-events-none absolute inset-0 text-black'
      data-layer='text-blocks'
    >
      {textBlocks.map((block, index) => (
        <BlockText
          key={`${block.x}-${block.y}-${index}`}
          block={block}
          scaleRatio={scaleRatio}
        />
      ))}
    </div>
  )
}

type BlockTextProps = {
  block: TextBlock
  scaleRatio: number
}

function BlockText({ block, scaleRatio }: BlockTextProps) {
  const content = block.translation?.trim()

  if (!content) {
    return null
  }

  const width = Math.max(0, block.width * scaleRatio)
  const height = Math.max(0, block.height * scaleRatio)

  if (!width || !height) {
    return null
  }

  const blockStyles = {
    left: block.x * scaleRatio,
    top: block.y * scaleRatio,
    width,
    height,
  } as CSSProperties

  return (
    <div className='absolute overflow-hidden' style={blockStyles}>
      <AutoTextSize
        mode='box'
        className='size-full text-center leading-tight font-semibold whitespace-pre-wrap text-black [writing-mode:vertical-rl]'
      >
        {content}
      </AutoTextSize>
    </div>
  )
}
