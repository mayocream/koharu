'use client'

import { useEffect } from 'react'
import { Rnd, type RndResizeCallback, type RndDragCallback } from 'react-rnd'
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
  const { textBlocks, replaceBlock, removeBlock } = useTextBlocks()
  const mode = useAppStore((state) => state.mode)
  const interactive = mode === 'select'

  useEffect(() => {
    if (!interactive) return

    const handleKeyDown = (event: KeyboardEvent) => {
      const isDeleteKey = event.key === 'Delete' || event.key === 'Backspace'
      if (!isDeleteKey) return

      const target = event.target as HTMLElement | null
      const isEditable = target?.closest('input, textarea, [contenteditable]')
      if (isEditable) return
      if (selectedIndex === undefined) return

      event.preventDefault()
      void removeBlock(selectedIndex)
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => {
      window.removeEventListener('keydown', handleKeyDown)
    }
  }, [interactive, removeBlock, selectedIndex])

  return (
    <div
      className='absolute inset-0'
      data-annotation-layer
      style={{ pointerEvents: interactive ? 'auto' : 'none' }}
    >
      {textBlocks.map((block, index) => (
        <TextBlockAnnotation
          key={`${block.x}-${block.y}-${index}`}
          block={block}
          index={index}
          selected={index === selectedIndex}
          onSelect={onSelect}
          interactive={interactive}
          onUpdate={(updates) => void replaceBlock(index, updates)}
        />
      ))}
    </div>
  )
}

type TextBlockAnnotationProps = {
  block: TextBlock
  index: number
  selected: boolean
  interactive: boolean
  onSelect: (index: number) => void
  onUpdate: (updates: Partial<TextBlock>) => void
}

function TextBlockAnnotation({
  block,
  index,
  selected,
  interactive,
  onSelect,
  onUpdate,
}: TextBlockAnnotationProps) {
  const scale = useAppStore((state) => state.scale)
  const scaleRatio = scale / 100

  const size = {
    width: Math.max(0, block.width * scaleRatio),
    height: Math.max(0, block.height * scaleRatio),
  }

  const position = {
    x: block.x * scaleRatio,
    y: block.y * scaleRatio,
  }

  const handleDragStop: RndDragCallback = (_, data) => {
    if (!interactive || !selected) return
    onUpdate({
      x: Math.round(data.x / scaleRatio),
      y: Math.round(data.y / scaleRatio),
    })
  }

  const handleResizeStop: RndResizeCallback = (_, __, ref, ___, position) => {
    if (!interactive || !selected) return
    const widthPx = parseFloat(ref.style.width)
    const heightPx = parseFloat(ref.style.height)
    onUpdate({
      x: Math.round(position.x / scaleRatio),
      y: Math.round(position.y / scaleRatio),
      width: Math.max(4, Math.round(widthPx / scaleRatio)),
      height: Math.max(4, Math.round(heightPx / scaleRatio)),
    })
  }

  return (
    <Rnd
      size={size}
      position={position}
      bounds='parent'
      disableDragging={!selected || !interactive}
      enableResizing={
        selected && interactive
          ? {
              bottom: true,
              bottomLeft: true,
              bottomRight: true,
              left: true,
              right: true,
              top: true,
              topLeft: true,
              topRight: true,
            }
          : false
      }
      onDragStop={handleDragStop}
      onResizeStop={handleResizeStop}
      onMouseDown={(event) => {
        if (!interactive) return
        event.stopPropagation()
        onSelect(index)
      }}
      style={{
        zIndex: selected ? 20 : 10,
        pointerEvents: interactive ? 'auto' : 'none',
      }}
      className='absolute'
    >
      <div className='relative h-full w-full select-none'>
        <div
          className={`absolute inset-0 rounded border ${
            selected ? 'border-sky-500' : 'border-rose-500/70'
          }`}
          style={{
            borderWidth: 2,
            backgroundColor: selected
              ? 'rgba(59, 130, 246, 0.08)'
              : 'rgba(244, 63, 94, 0.08)',
          }}
        />
        <div
          className={`pointer-events-none absolute -top-1.5 -left-1.5 flex h-4 w-4 items-center justify-center rounded-full text-[9px] font-semibold text-white shadow ${
            selected ? 'bg-sky-500' : 'bg-rose-500'
          }`}
        >
          {index + 1}
        </div>
      </div>
    </Rnd>
  )
}
