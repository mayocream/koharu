'use client'

import { useEffect, useRef, useState } from 'react'
import { Rnd, type RndResizeCallback, type RndDragCallback } from 'react-rnd'
import { useHotkeys } from 'react-hotkeys-hook'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { TextBlock } from '@/types'
import { useTextBlocks } from '@/hooks/useTextBlocks'

type TextBlockAnnotationsProps = {
  selectedIndex?: number
  selectedIndices?: number[]
  onSelect: (index?: number) => void
  onToggleSelect?: (index: number) => void
  style?: React.CSSProperties
}

export function TextBlockAnnotations({
  selectedIndex,
  selectedIndices = [],
  onSelect,
  onToggleSelect,
  style,
}: TextBlockAnnotationsProps) {
  const { textBlocks, replaceBlock, removeBlock, commitDragUndo } =
    useTextBlocks()
  const mode = useEditorUiStore((state) => state.mode)
  const interactive = mode === 'select' || mode === 'block'

  useHotkeys(
    'backspace,delete',
    (event) => {
      if (!interactive || selectedIndex === undefined) return
      const target = event.target as HTMLElement | null
      const isEditable = target?.closest('input, textarea, [contenteditable]')
      if (isEditable) return
      event.preventDefault()
      void removeBlock(selectedIndex)
    },
    {
      enabled: interactive,
      preventDefault: true,
      enableOnFormTags: false,
    },
    [interactive, removeBlock, selectedIndex],
  )

  return (
    <div
      data-testid='workspace-annotations'
      className='absolute inset-0'
      data-annotation-layer
      style={{
        ...style,
        pointerEvents: 'none',
      }}
    >
      {textBlocks.map((block, index) => (
        <TextBlockAnnotation
          key={block.id ?? index}
          block={block}
          index={index}
          selected={index === selectedIndex}
          multiSelected={selectedIndices.includes(index)}
          onSelect={onSelect}
          onToggleSelect={onToggleSelect}
          interactive={interactive}
          onUpdate={(updates) => void replaceBlock(index, updates)}
          onDragEnd={commitDragUndo}
        />
      ))}
    </div>
  )
}

type TextBlockAnnotationProps = {
  block: TextBlock
  index: number
  selected: boolean
  multiSelected: boolean
  interactive: boolean
  onSelect: (index: number) => void
  onToggleSelect?: (index: number) => void
  onUpdate: (updates: Partial<TextBlock>) => void
  onDragEnd: () => void
}

function TextBlockAnnotation({
  block,
  index,
  selected,
  multiSelected,
  interactive,
  onSelect,
  onToggleSelect,
  onUpdate,
  onDragEnd,
}: TextBlockAnnotationProps) {
  const scale = useEditorUiStore((state) => state.scale)
  const scaleRatio = scale / 100
  const draggingRef = useRef(false)

  const scaledSize = {
    width: Math.max(0, block.width * scaleRatio),
    height: Math.max(0, block.height * scaleRatio),
  }

  const scaledPosition = {
    x: block.x * scaleRatio,
    y: block.y * scaleRatio,
  }

  const [size, setSize] = useState(scaledSize)
  const [position, setPosition] = useState(scaledPosition)

  // Sync from props, but NOT while the user is actively dragging.
  useEffect(() => {
    if (draggingRef.current) return
    setSize(scaledSize)
    setPosition(scaledPosition)
  }, [scaledPosition.x, scaledPosition.y, scaledSize.width, scaledSize.height])

  const handleDrag: RndDragCallback = (_, data) => {
    if (!interactive) return
    setPosition({ x: data.x, y: data.y })
  }

  const handleDragStop: RndDragCallback = (_, data) => {
    if (!interactive) return
    draggingRef.current = false
    const nextPosition = { x: data.x, y: data.y }
    setPosition(nextPosition)
    onUpdate({
      x: Math.round(nextPosition.x / scaleRatio),
      y: Math.round(nextPosition.y / scaleRatio),
    })
    // Commit the undo entry for the entire drag session
    onDragEnd()
  }

  const handleResize: RndResizeCallback = (_, __, ref, ___, nextPosition) => {
    if (!interactive || !selected) return
    setSize({
      width: parseFloat(ref.style.width),
      height: parseFloat(ref.style.height),
    })
    setPosition(nextPosition)
  }

  const handleResizeStop: RndResizeCallback = (_, __, ref, ___, pos) => {
    if (!interactive || !selected) return
    draggingRef.current = false
    const widthPx = parseFloat(ref.style.width)
    const heightPx = parseFloat(ref.style.height)
    const nextSize = {
      width: widthPx,
      height: heightPx,
    }
    setSize(nextSize)
    setPosition(pos)
    onUpdate({
      x: Math.round(pos.x / scaleRatio),
      y: Math.round(pos.y / scaleRatio),
      width: Math.max(4, Math.round(nextSize.width / scaleRatio)),
      height: Math.max(4, Math.round(nextSize.height / scaleRatio)),
    })
  }

  const handlePointerDown = (event: MouseEvent) => {
    if (!interactive) return
    event.stopPropagation()
    if (event.ctrlKey || event.metaKey) {
      onToggleSelect?.(index)
    } else {
      onSelect(index)
    }
  }

  return (
    <Rnd
      size={size}
      position={position}
      bounds='parent'
      disableDragging={!interactive}
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
      onDragStart={() => {
        if (!interactive) return
        draggingRef.current = true
        onSelect(index)
      }}
      onDrag={handleDrag}
      onDragStop={handleDragStop}
      onResizeStart={() => {
        if (!interactive) return
        draggingRef.current = true
        onSelect(index)
      }}
      onResize={handleResize}
      onResizeStop={handleResizeStop}
      onMouseDown={handlePointerDown}
      onPointerDown={handlePointerDown}
      style={{
        zIndex: selected ? 20 : multiSelected ? 15 : 10,
        pointerEvents: interactive ? 'auto' : 'none',
        willChange: 'transform',
      }}
      className='absolute'
    >
      <div className='relative h-full w-full select-none'>
        <div
          className={`absolute inset-0 rounded ${
            selected
              ? 'border-primary bg-primary/15 border-[3px]'
              : multiSelected
                ? 'border-[3px] border-sky-400 bg-sky-400/15'
                : 'border-2 border-rose-400/60 bg-rose-400/5'
          }`}
        />
        <div
          className={`pointer-events-none absolute -top-1.5 -left-1.5 flex h-4 w-4 items-center justify-center rounded-full text-[9px] font-semibold text-white shadow ${
            selected
              ? 'bg-primary'
              : multiSelected
                ? 'bg-sky-400'
                : 'bg-rose-400'
          }`}
        >
          {index + 1}
        </div>
      </div>
    </Rnd>
  )
}
