'use client'

import {
  PropsWithChildren,
  useCallback,
  useEffect,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from 'react'

type ResizableSidebarProps = PropsWithChildren<{
  side: 'left' | 'right'
  initialWidth: number
  minWidth?: number
  maxWidth?: number
  className?: string
}>

const clamp = (value: number, min?: number, max?: number) => {
  let next = value
  if (typeof min === 'number') {
    next = Math.max(min, next)
  }
  if (typeof max === 'number') {
    next = Math.min(max, next)
  }
  return next
}

export function ResizableSidebar({
  side,
  initialWidth,
  minWidth,
  maxWidth,
  className,
  children,
}: ResizableSidebarProps) {
  const [width, setWidth] = useState(initialWidth)
  const [dragging, setDragging] = useState(false)
  const dragState = useRef<{ startX: number; startWidth: number } | null>(null)

  useEffect(() => {
    setWidth(initialWidth)
  }, [initialWidth])

  const handlePointerMove = useCallback(
    (event: PointerEvent) => {
      if (!dragState.current) return
      event.preventDefault()
      const delta = event.clientX - dragState.current.startX
      const direction = side === 'left' ? 1 : -1
      const nextWidth = clamp(
        dragState.current.startWidth + delta * direction,
        minWidth,
        maxWidth,
      )
      setWidth(nextWidth)
    },
    [maxWidth, minWidth, side],
  )

  const stopDragging = useCallback(() => {
    dragState.current = null
    setDragging(false)
    document.body.style.userSelect = ''
    document.body.style.cursor = ''
    window.removeEventListener('pointermove', handlePointerMove)
    window.removeEventListener('pointerup', stopDragging)
  }, [handlePointerMove])

  const handlePointerDown = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (event.button !== 0 && event.pointerType !== 'touch') return
    event.preventDefault()
    dragState.current = { startX: event.clientX, startWidth: width }
    setDragging(true)
    document.body.style.userSelect = 'none'
    document.body.style.cursor = 'col-resize'
    window.addEventListener('pointermove', handlePointerMove)
    window.addEventListener('pointerup', stopDragging)
  }

  useEffect(() => {
    return () => {
      stopDragging()
    }
  }, [stopDragging])

  const handlePosition = side === 'left' ? 'right-[-3px]' : 'left-[-3px]' // nudged inward for easier grabbing

  const containerClassName = [
    'relative flex h-full min-h-0 shrink-0',
    className ?? '',
  ]
    .filter(Boolean)
    .join(' ')

  return (
    <div className={containerClassName} style={{ width }}>
      <div className='flex h-full w-full flex-col'>{children}</div>
      <div
        role='separator'
        aria-orientation='vertical'
        data-dragging={dragging}
        onPointerDown={handlePointerDown}
        className={`absolute top-0 h-full w-1.5 cursor-col-resize touch-none transition-colors select-none ${handlePosition} hover:bg-neutral-200 data-[dragging=true]:bg-neutral-300`}
      />
    </div>
  )
}
