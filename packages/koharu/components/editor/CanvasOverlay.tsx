'use client'

import { useLayoutEffect, useMemo, useRef, useState } from 'react'
import { flushSync } from 'react-dom'
import Moveable, {
  type OnResize,
  type OnResizeEnd,
  type OnResizeStart,
  type OnRotate,
  type OnRotateEnd,
  type OnRotateStart,
} from 'react-moveable'

import { isTextLayer } from '@/lib/document'
import { controlFrame, cssFrame, selectableLayer, type Camera } from '@/lib/geometry'
import type { EntityId, Frame, Page, Point, TransformFrame } from '@/lib/protocol'

interface CanvasOverlayProps {
  page: Page
  camera: Camera
  selected: EntityId[]
  hovered: EntityId | null
  frames: Readonly<Record<EntityId, Frame>>
  previews: Readonly<Record<EntityId, Frame>>
  draft: Frame | null
  cursor: Point | null
  brushSize: number
  showBrushCursor: boolean
  onTransformStart: (elements: TransformFrame[]) => void
  onTransformFrame: (elements: TransformFrame[]) => void
  onTransformEnd: () => void
}

interface ControlGesture {
  layer: EntityId
  original: Frame
}

export function CanvasOverlay({
  page,
  camera,
  selected,
  hovered,
  frames,
  previews,
  draft,
  cursor,
  brushSize,
  showBrushCursor,
  onTransformStart,
  onTransformFrame,
  onTransformEnd,
}: CanvasOverlayProps) {
  const root = useRef<HTMLDivElement>(null)
  const moveable = useRef<Moveable | null>(null)
  const gesture = useRef<ControlGesture | null>(null)
  const [targets, setTargets] = useState<HTMLElement[]>([])
  const selectedIds = useMemo(() => new Set(selected), [selected])
  const layers = useMemo(
    () =>
      page.layers.flatMap((layer) => {
        if (!layer.visibility.visible || layer.visibility.opacity <= 0) return []
        const frame = previews[layer.id] ?? controlFrame(layer, frames)
        return frame ? [{ layer, frame }] : []
      }),
    [page.layers, previews, frames],
  )
  const cameraX = camera.translation[0]
  const cameraY = camera.translation[1]

  useLayoutEffect(() => {
    setTargets(
      root.current
        ? Array.from(root.current.querySelectorAll<HTMLElement>('[data-moveable-target]'))
        : [],
    )
  }, [page.id, selected])

  useLayoutEffect(() => {
    moveable.current?.updateRect()
  }, [camera.zoom, cameraX, cameraY, layers, targets])

  useLayoutEffect(() => {
    const element = root.current
    if (!element) return
    const resize = new ResizeObserver(() => moveable.current?.updateRect())
    resize.observe(element)
    return () => resize.disconnect()
  }, [])

  const selectedText =
    selected.length === 1 &&
    page.layers.some((layer) => layer.id === selected[0] && isTextLayer(layer))
  const scale = camera.zoom / window.devicePixelRatio

  const start = (target: HTMLElement): ControlGesture | null => {
    const id = target.dataset.element
    const current = id && layers.find(({ layer }) => layer.id === id)
    if (!current) return null
    const next = { layer: current.layer.id, original: current.frame }
    gesture.current = next
    onTransformStart([{ element: next.layer, frame: next.original }])
    return next
  }

  const resizeStart = (event: OnResizeStart) => {
    const current = start(event.target as HTMLElement)
    if (!current) return
    event.set([event.target.clientWidth, event.target.clientHeight])
    if (event.dragStart) event.dragStart.set([0, 0])
  }

  const resize = (event: OnResize) => {
    const current = gesture.current
    if (!current || scale <= 0) return
    const frame: Frame = {
      ...current.original,
      x: current.original.x + event.drag.beforeTranslate[0] / scale,
      y: current.original.y + event.drag.beforeTranslate[1] / scale,
      width: Math.max(1 / scale, event.width / scale),
      height: Math.max(1 / scale, event.height / scale),
    }
    applyFrame(event.target as HTMLElement, frame, camera)
    onTransformFrame([{ element: current.layer, frame }])
  }

  const rotateStart = (event: OnRotateStart) => {
    const current = start(event.target as HTMLElement)
    if (!current) return
    event.set(current.original.angle_degrees)
    if (event.dragStart) event.dragStart.set([0, 0])
  }

  const rotate = (event: OnRotate) => {
    const current = gesture.current
    if (!current) return
    const frame = { ...current.original, angle_degrees: event.beforeRotation }
    applyFrame(event.target as HTMLElement, frame, camera)
    onTransformFrame([{ element: current.layer, frame }])
  }

  const end = (_event: OnResizeEnd | OnRotateEnd) => {
    if (!gesture.current) return
    gesture.current = null
    onTransformEnd()
  }

  return (
    <div ref={root} className='pointer-events-none absolute inset-0 overflow-hidden' aria-hidden>
      {layers.map(({ layer, frame }) => {
        const position = cssFrame(frame, camera)
        const selected = selectedIds.has(layer.id) && selectableLayer(layer)
        const highlighted = !selected && hovered === layer.id
        return (
          <div
            key={layer.id}
            data-element={layer.id}
            data-moveable-target={selected || undefined}
            className='absolute box-border bg-transparent'
            style={{
              left: position.left,
              top: position.top,
              width: position.width,
              height: position.height,
              transform: `rotate(${position.angle}deg)`,
              transformOrigin: '50% 50%',
              border: highlighted ? '1px solid var(--primary)' : undefined,
              opacity: layer.visibility.opacity,
              willChange: selected ? 'left, top, width, height, transform' : undefined,
            }}
          />
        )
      })}

      {draft && <DraftOverlay frame={draft} camera={camera} />}
      {showBrushCursor && cursor && (
        <div
          className='absolute rounded-full border border-foreground/90 shadow-[0_0_0_1px_rgb(0_0_0/0.4)]'
          style={{
            left: cursor.x / window.devicePixelRatio - (brushSize * scale) / 2,
            top: cursor.y / window.devicePixelRatio - (brushSize * scale) / 2,
            width: brushSize * scale,
            height: brushSize * scale,
          }}
        />
      )}

      <Moveable
        ref={moveable}
        target={targets.length === 1 ? targets[0] : targets}
        flushSync={flushSync}
        className={selectedText ? 'koharu-moveable koharu-moveable-text' : 'koharu-moveable'}
        draggable={false}
        resizable={targets.length === 1}
        rotatable={targets.length === 1}
        origin={false}
        throttleResize={0}
        throttleRotate={0}
        renderDirections={selectedText ? false : ['nw', 'n', 'ne', 'e', 'se', 's', 'sw', 'w']}
        edge={selectedText}
        rotationPosition='top'
        onResizeStart={resizeStart}
        onResize={resize}
        onResizeEnd={end}
        onRotateStart={rotateStart}
        onRotate={rotate}
        onRotateEnd={end}
      />
    </div>
  )
}

function DraftOverlay({ frame, camera }: { frame: Frame; camera: Camera }) {
  const position = cssFrame(frame, camera)
  return (
    <div
      className='absolute border border-dashed border-primary bg-primary/5'
      style={{
        left: position.left,
        top: position.top,
        width: position.width,
        height: position.height,
        transform: `rotate(${position.angle}deg)`,
      }}
    />
  )
}

function applyFrame(target: HTMLElement, frame: Frame, camera: Camera) {
  const position = cssFrame(frame, camera)
  Object.assign(target.style, {
    left: `${position.left}px`,
    top: `${position.top}px`,
    width: `${position.width}px`,
    height: `${position.height}px`,
    transform: `rotate(${position.angle}deg)`,
  })
}
