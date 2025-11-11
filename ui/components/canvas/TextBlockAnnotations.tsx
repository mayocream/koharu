'use client'

import { useEffect, useRef } from 'react'
import type Konva from 'konva'
import { Rect, Transformer, Circle, Text } from 'react-konva'
import { useAppStore } from '@/lib/store'
import { TextBlock } from '@/types'
import { useTextBlocks } from '@/hooks/useTextBlocks'

type TextBlockAnnotationsProps = {
  selectedIndex?: number
  onSelect: (index?: number) => void
}

export function TextBlockAnnotations({
  selectedIndex,
  onSelect,
}: TextBlockAnnotationsProps) {
  const { textBlocks, replaceBlock } = useTextBlocks()

  return (
    <>
      {textBlocks.map((block, index) => (
        <TextBlockAnnotation
          key={`${block.x}-${block.y}-${index}`}
          block={block}
          index={index}
          selected={index === selectedIndex}
          onSelect={onSelect}
          onUpdate={(updates) => void replaceBlock(index, updates)}
        />
      ))}
    </>
  )
}

type TextBlockAnnotationProps = {
  block: TextBlock
  index: number
  selected: boolean
  onSelect: (index: number) => void
  onUpdate: (updates: Partial<TextBlock>) => void
}

function TextBlockAnnotation({
  block,
  index,
  selected,
  onSelect,
  onUpdate,
}: TextBlockAnnotationProps) {
  const scale = useAppStore((state) => state.scale)
  const scaleRatio = scale / 100
  const rectRef = useRef<Konva.Rect>(null)
  const transformerRef = useRef<Konva.Transformer>(null)

  useEffect(() => {
    if (!selected || !transformerRef.current || !rectRef.current) return
    transformerRef.current.nodes([rectRef.current])
    transformerRef.current.getLayer()?.batchDraw()
  }, [selected])

  const handleTransformEnd = () => {
    const node = rectRef.current
    if (!node) return

    const scaleX = node.scaleX()
    const scaleY = node.scaleY()
    const width = Math.max(4, node.width() * scaleX)
    const height = Math.max(4, node.height() * scaleY)

    node.scaleX(1)
    node.scaleY(1)

    onUpdate({
      x: Math.round(node.x()),
      y: Math.round(node.y()),
      width: Math.round(width),
      height: Math.round(height),
    })
  }

  return (
    <>
      <Rect
        ref={rectRef}
        x={block.x}
        y={block.y}
        width={block.width}
        height={block.height}
        stroke={selected ? 'rgba(59, 130, 246, 0.9)' : 'rgba(255, 0, 0, 0.5)'}
        strokeWidth={2 / scaleRatio}
        onClick={(event) => {
          event.cancelBubble = true
          onSelect(index)
        }}
        onTransformEnd={handleTransformEnd}
        listening
      />
      {selected && (
        <Transformer
          ref={transformerRef}
          rotateEnabled={false}
          boundBoxFunc={(oldBox, newBox) => {
            if (newBox.width < 8 || newBox.height < 8) {
              return oldBox
            }
            return newBox
          }}
        />
      )}
      <Circle
        x={block.x}
        y={block.y}
        radius={9 / scaleRatio}
        fill={selected ? 'rgba(59, 130, 246, 0.9)' : 'rgba(255, 0, 0, 0.7)'}
      />
      <Text
        x={block.x - (index + 1 >= 10 ? 6 : 4) / scaleRatio}
        y={block.y - 6 / scaleRatio}
        text={(index + 1).toString()}
        fontSize={12 / scaleRatio}
        fill='white'
      />
    </>
  )
}
