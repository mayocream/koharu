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

import { isTextElement, type EntityId, type Frame, type PageView, type TransformFrame } from '@/lib/koharu'
import { cssFrame, entityFrame } from '@/lib/koharu/geometry'

interface CanvasOverlayProps {
  page: PageView
  camera: { zoom: number; translation: [number, number] }
  selected: EntityId[]
  hovered: EntityId | null
  previews: Readonly<Record<EntityId, Frame>>
  draft: Frame | null
  cursor: [number, number] | null
  brushSize: number
  showBrushCursor: boolean
  onTransformStart: (elements: TransformFrame[]) => void
  onTransformFrame: (elements: TransformFrame[]) => void
  onTransformEnd: () => void
}

interface ControlGesture {
  element: EntityId
  original: Frame
}

export function CanvasOverlay({
  page,
  camera,
  selected,
  hovered,
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
  const gesture = useRef<ControlGesture | null>(null)
  const [targets, setTargets] = useState<HTMLElement[]>([])
  const selectedIds = useMemo(() => new Set(selected), [selected])
  const entities = useMemo(
    () =>
      page.entities.flatMap((entity, index) => {
        if (!entity.visibility.visible || entity.visibility.opacity <= 0) return []
        const frame = previews[entity.id] ?? entityFrame(entity)
        return frame ? [{ entity, frame, index }] : []
      }),
    [page.entities, previews],
  )

  const selectable = (entity: PageView['entities'][number]) =>
    entity.image !== null || isTextElement(entity)

  useLayoutEffect(() => {
    setTargets(
      root.current
        ? Array.from(root.current.querySelectorAll<HTMLElement>('[data-moveable-target]'))
        : [],
    )
  }, [page.id, selected])

  const selectedText =
    selected.length === 1 &&
    page.entities.some((entity) => entity.id === selected[0] && isTextElement(entity))
  const scale = camera.zoom / window.devicePixelRatio

  function start(target: HTMLElement): ControlGesture | null {
    const element = target.dataset.element
    const current = element && entities.find((item) => item.entity.id === element)
    if (!current) return null
    const next = { element: current.entity.id, original: current.frame }
    gesture.current = next
    onTransformStart([{ element: next.element, frame: next.original }])
    return next
  }

  function resizeStart(event: OnResizeStart) {
    const current = start(event.target as HTMLElement)
    if (!current) return
    event.set([event.target.clientWidth, event.target.clientHeight])
    if (event.dragStart) event.dragStart.set([0, 0])
  }

  function resize(event: OnResize) {
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
    onTransformFrame([{ element: current.element, frame }])
  }

  function rotateStart(event: OnRotateStart) {
    const current = start(event.target as HTMLElement)
    if (!current) return
    event.set(current.original.angle_degrees)
    if (event.dragStart) event.dragStart.set([0, 0])
  }

  function rotate(event: OnRotate) {
    const current = gesture.current
    if (!current) return
    const frame = { ...current.original, angle_degrees: event.beforeRotation }
    applyFrame(event.target as HTMLElement, frame, camera)
    onTransformFrame([{ element: current.element, frame }])
  }

  function end(_event: OnResizeEnd | OnRotateEnd) {
    if (!gesture.current) return
    gesture.current = null
    onTransformEnd()
  }

  return (
    <div ref={root} className='pointer-events-none absolute inset-0 overflow-hidden' aria-hidden>
      {entities.map(({ entity, frame, index }) => {
        const position = cssFrame(frame, camera)
        const text = isTextElement(entity)
        const selected = selectedIds.has(entity.id) && selectable(entity)
        const showOutline = text || selected || hovered === entity.id
        return (
          <div
            key={entity.id}
            data-element={entity.id}
            data-moveable-target={selected || undefined}
            className='absolute box-border bg-transparent'
            style={{
              left: position.left,
              top: position.top,
              width: position.width,
              height: position.height,
              transform: `rotate(${position.angle}deg)`,
              transformOrigin: '50% 50%',
              border: showOutline
                ? `${selected ? 2 : 1}px solid ${text ? 'rgb(244 63 94)' : 'rgb(34 211 238)'}`
                : undefined,
              opacity: entity.visibility.opacity,
              willChange: selected ? 'left, top, width, height, transform' : undefined,
            }}
          >
            {text && (
              <span className='absolute -left-3 -top-3 grid size-6 place-items-center rounded-full border border-rose-400 bg-background/90 text-[10px] font-semibold text-rose-500'>
                {index + 1}
              </span>
            )}
          </div>
        )
      })}

      {draft && <DraftOverlay frame={draft} camera={camera} />}
      {showBrushCursor && cursor && (
        <div
          className='absolute rounded-full border border-white/90 shadow-[0_0_0_1px_rgb(0_0_0/0.35)]'
          style={{
            left: cursor[0] / window.devicePixelRatio - (brushSize * scale) / 2,
            top: cursor[1] / window.devicePixelRatio - (brushSize * scale) / 2,
            width: brushSize * scale,
            height: brushSize * scale,
          }}
        />
      )}

      <Moveable
        target={targets.length === 1 ? targets[0] : targets}
        flushSync={flushSync}
        className={selectedText ? 'koharu-moveable koharu-moveable-text' : 'koharu-moveable'}
        draggable={false}
        resizable={targets.length === 1}
        rotatable={targets.length === 1}
        origin={false}
        throttleResize={0}
        throttleRotate={0}
        renderDirections={
          selectedText ? ['n', 'ne', 'e', 'se', 's', 'sw', 'w'] : ['nw', 'n', 'ne', 'e', 'se', 's', 'sw', 'w']
        }
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

function DraftOverlay({
  frame,
  camera,
}: {
  frame: Frame
  camera: { zoom: number; translation: [number, number] }
}) {
  const position = cssFrame(frame, camera)
  return (
    <div
      className='absolute border border-dashed border-sky-400 bg-sky-400/5'
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

function applyFrame(
  target: HTMLElement,
  frame: Frame,
  camera: { zoom: number; translation: [number, number] },
) {
  const position = cssFrame(frame, camera)
  Object.assign(target.style, {
    left: `${position.left}px`,
    top: `${position.top}px`,
    width: `${position.width}px`,
    height: `${position.height}px`,
    transform: `rotate(${position.angle}deg)`,
  })
}
