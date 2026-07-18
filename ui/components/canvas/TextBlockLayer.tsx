'use client'

import { useDrag } from '@use-gesture/react'
import { useCallback, useEffect, useRef } from 'react'

import { useBlobImage } from '@/hooks/useBlobData'
import { useCurrentPage, useTextNodes, type TextNodeEntry } from '@/hooks/useCurrentPage'
import type { NodeDataPatch, Transform } from '@/lib/api/schemas'
import { applyOp, queueAutoRender } from '@/lib/io/scene'
import { ops } from '@/lib/ops'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { useSelectionStore } from '@/lib/stores/selectionStore'

type TextBlockLayerProps = {
  showSprites?: boolean
  scale: number
  style?: React.CSSProperties
}

type BoxGeometry = {
  x: number
  y: number
  width: number
  height: number
}

// Resize cursors rotated in 45° steps. Index = (deg/45) % 8.
const ROTATE_CURSOR_MAP: Record<string, readonly string[]> = {
  'ns-resize':   ['ns-resize',   'nesw-resize', 'ew-resize',   'nwse-resize', 'ns-resize',   'nesw-resize', 'ew-resize',   'nwse-resize'],
  'ew-resize':   ['ew-resize',   'nwse-resize', 'ns-resize',   'nesw-resize', 'ew-resize',   'nwse-resize', 'ns-resize',   'nesw-resize'],
  'nwse-resize': ['nwse-resize', 'ns-resize',   'nesw-resize', 'ew-resize',   'nwse-resize', 'ns-resize',   'nesw-resize', 'ew-resize'],
  'nesw-resize': ['nesw-resize', 'ew-resize',   'nwse-resize', 'ns-resize',   'nesw-resize', 'ew-resize',   'nwse-resize', 'ns-resize'],
}

const rotateCursor = (cursor: string, deg: number): string => {
  const steps = Math.round(((deg % 360) + 360) % 360 / 45) % 8
  return ROTATE_CURSOR_MAP[cursor]?.[steps] ?? cursor
}

/**
 * Overlay for the active page's Text nodes. Each rectangle is draggable /
 * resizable; commits dispatch `Op::UpdateNode { transform }` through
 * `applyCommand`. Selection is driven by `selectionStore.nodeIds`.
 */
export function TextBlockLayer({ showSprites, scale, style }: TextBlockLayerProps) {
  const nodes = useTextNodes()
  const page = useCurrentPage()
  const selectedIds = useSelectionStore((s) => s.nodeIds)
  const select = useSelectionStore((s) => s.select)
  const mode = useEditorUiStore((s) => s.mode)
  const interactive = mode === 'select' || mode === 'block'

  const updateTransform = useCallback(
    async (id: string, geometry: BoxGeometry) => {
      if (!page) return
      const rotationDeg = page.nodes[id]?.transform?.rotationDeg ?? 0
      const next: Transform = {
        x: geometry.x,
        y: geometry.y,
        width: geometry.width,
        height: geometry.height,
        rotationDeg,
      }
      const data: NodeDataPatch = {
        text: {
          lockLayoutBox: true,
        },
      }
      await applyOp(ops.updateNode(page.id, id, { transform: next, data }))
      queueAutoRender(page.id)
    },
    [page, applyOp, queueAutoRender],
  )

  return (
    <div
      data-text-block-layer
      style={{
        ...style,
        position: 'absolute',
        inset: 0,
        width: '100%',
        height: '100%',
        pointerEvents: 'none',
      }}
    >
      {showSprites &&
        nodes.map((n, i) => <BlockSprite key={`sprite-${n.id ?? i}`} node={n} scale={scale} />)}
      {nodes.map((n, i) => (
        <TextBlockItem
          key={n.id}
          node={n}
          index={i}
          scale={scale}
          selected={selectedIds.has(n.id)}
          interactive={interactive}
          onSelect={(id, additive) => select(id, additive)}
          onCommit={(geometry) => void updateTransform(n.id, geometry)}
        />
      ))}
    </div>
  )
}

type TextBlockItemProps = {
  node: TextNodeEntry
  index: number
  scale: number
  selected: boolean
  interactive: boolean
  onSelect: (id: string, additive: boolean) => void
  onCommit: (geometry: BoxGeometry) => void
}

const isAdditiveEvent = (event: unknown): boolean => {
  if (!event || typeof event !== 'object') return false
  const e = event as { shiftKey?: boolean; metaKey?: boolean; ctrlKey?: boolean }
  return !!(e.shiftKey || e.metaKey || e.ctrlKey)
}

const RESIZE_HANDLE_SIZE = 8

type ResizeEdge = { top: boolean; bottom: boolean; left: boolean; right: boolean }

function TextBlockItem({
  node,
  index,
  scale,
  selected,
  interactive,
  onSelect,
  onCommit,
}: TextBlockItemProps) {
  const boxRef = useRef<HTMLDivElement>(null)
  const dragStart = useRef({ x: 0, y: 0, w: 0, h: 0 })
  const edgeRef = useRef<ResizeEdge | null>(null)
  const isResizeRef = useRef(false)

  useEffect(() => {
    if (selected && boxRef.current) {
      boxRef.current.focus()
    }
  }, [selected])

  const setBox = (x: number, y: number, w: number, h: number) => {
    const el = boxRef.current
    if (!el) return
    el.style.transform = `translate(${x}px, ${y}px) rotate(${t.rotationDeg ?? 0}deg)`
    el.style.width = `${w}px`
    el.style.height = `${h}px`
  }

  const t = node.transform

  const bind = useDrag(
    ({ first, last, movement: [mx, my], event, tap }) => {
      if (!interactive) return
      event?.stopPropagation()
      const additive = isAdditiveEvent(event)
      if (tap) {
        onSelect(node.id, additive)
        boxRef.current?.focus()
        return
      }
      if (first) {
        dragStart.current = {
          x: t.x * scale,
          y: t.y * scale,
          w: t.width * scale,
          h: t.height * scale,
        }
        // Keep multi-selection intact when dragging a node that's already selected;
        // otherwise this click is a single-select (unless the modifier is held).
        if (additive || !selected) onSelect(node.id, additive)
        boxRef.current?.focus()
      }
      const { x: sx, y: sy, w: sw, h: sh } = dragStart.current
      const edge = edgeRef.current
      if (isResizeRef.current && edge) {
        const rotationRad = ((t.rotationDeg ?? 0) * Math.PI) / 180
        const cos = Math.cos(rotationRad)
        const sin = Math.sin(rotationRad)

        // Project pointer delta to box-local space so resize feels natural under rotation.
        const localDx = mx * cos + my * sin
        const localDy = -mx * sin + my * cos

        let moveLocalX = 0
        let moveLocalY = 0
        let w = sw
        let h = sh
        if (edge.right) w += localDx
        if (edge.left) {
          w -= localDx
          moveLocalX = localDx
        }
        if (edge.bottom) h += localDy
        if (edge.top) {
          h -= localDy
          moveLocalY = localDy
        }
        w = Math.max(4 * scale, w)
        h = Math.max(4 * scale, h)
        if (edge.left && w === 4 * scale) moveLocalX = sw - 4 * scale
        if (edge.top && h === 4 * scale) moveLocalY = sh - 4 * scale

        const worldDx = moveLocalX * cos - moveLocalY * sin
        const worldDy = moveLocalX * sin + moveLocalY * cos
        const nextX = sx + worldDx
        const nextY = sy + worldDy

        setBox(nextX, nextY, w, h)
        if (last) {
          isResizeRef.current = false
          edgeRef.current = null
          onCommit({
            x: Math.round(nextX / scale),
            y: Math.round(nextY / scale),
            width: Math.max(4, Math.round(w / scale)),
            height: Math.max(4, Math.round(h / scale)),
          })
        }
      } else {
        setBox(sx + mx, sy + my, sw, sh)
        if (last) {
          onCommit({
            x: Math.round((sx + mx) / scale),
            y: Math.round((sy + my) / scale),
            width: t.width,
            height: t.height,
          })
        }
      }
    },
    {
      pointer: { buttons: 1, touch: true },
      filterTaps: true,
      preventDefault: true,
      eventOptions: { passive: false },
    },
  )

  const handleEdgePointerDown = (edge: ResizeEdge) => {
    if (!interactive || !selected) return
    isResizeRef.current = true
    edgeRef.current = edge
  }

  const w = t.width * scale
  const h = t.height * scale

  return (
    <div
      ref={boxRef}
      {...bind()}
      tabIndex={-1}
      style={{
        position: 'absolute',
        top: 0,
        left: 0,
        transform: `translate(${t.x * scale}px, ${t.y * scale}px) rotate(${t.rotationDeg ?? 0}deg)`,
        transformOrigin: 'top left',
        width: w,
        height: h,
        pointerEvents: interactive ? 'auto' : 'none',
        zIndex: selected ? 20 : 10,
        touchAction: 'none',
        cursor: interactive ? 'move' : 'default',
        outline: 'none',
      }}
    >
      <div
        className={`absolute inset-0 rounded-md ${
          selected
            ? 'border-[3px] border-primary bg-primary/15'
            : 'border-2 border-rose-400/60 bg-rose-400/5'
        }`}
      />
      <div
        className={`pointer-events-none absolute -top-1.5 -left-1.5 flex h-4 w-4 items-center justify-center rounded-full text-[9px] font-semibold text-white shadow ${
          selected ? 'bg-primary' : 'bg-rose-400'
        }`}
        style={{ transform: `rotate(${t.rotationDeg ? -t.rotationDeg : 0}deg)` }}
      >
        {index + 1}
      </div>
      {selected && interactive && (
        <ResizeHandles
          onEdgePointerDown={handleEdgePointerDown}
          rotationDeg={t.rotationDeg ?? 0}
        />
      )}
    </div>
  )
}

function BlockSprite({ node, scale }: { node: TextNodeEntry; scale: number }) {
  const sprite = (node.data.sprite as string | null | undefined) ?? undefined
  const { data: src } = useBlobImage(sprite)
  if (!src) return null
  const spriteT = node.data.spriteTransform
  const x = (spriteT?.x ?? node.transform.x) * scale
  const y = (spriteT?.y ?? node.transform.y) * scale
  const rotation = spriteT?.rotationDeg ?? node.transform.rotationDeg ?? 0
  return (
    <img
      alt=''
      src={src}
      draggable={false}
      className='pointer-events-none absolute select-none'
      style={{
        top: 0,
        left: 0,
        transformOrigin: 'top left',
        transform: `translate(${x}px, ${y}px) rotate(${rotation}deg) scale(${scale})`,
      }}
    />
  )
}

function ResizeHandles({
  onEdgePointerDown,
  rotationDeg,
}: {
  onEdgePointerDown: (edge: ResizeEdge) => void
  rotationDeg: number
}) {
  const s = RESIZE_HANDLE_SIZE
  const half = s / 2

  const edges: { edge: ResizeEdge; style: React.CSSProperties; cursor: string }[] = [
    {
      edge: { top: true, left: true, bottom: false, right: false },
      cursor: 'nwse-resize',
      style: { top: -half, left: -half, width: s, height: s },
    },
    {
      edge: { top: true, left: false, bottom: false, right: true },
      cursor: 'nesw-resize',
      style: { top: -half, right: -half, width: s, height: s },
    },
    {
      edge: { top: false, left: true, bottom: true, right: false },
      cursor: 'nesw-resize',
      style: { bottom: -half, left: -half, width: s, height: s },
    },
    {
      edge: { top: false, left: false, bottom: true, right: true },
      cursor: 'nwse-resize',
      style: { bottom: -half, right: -half, width: s, height: s },
    },
    {
      edge: { top: true, left: false, bottom: false, right: false },
      cursor: 'ns-resize',
      style: { top: -half, left: s, right: s, height: s },
    },
    {
      edge: { top: false, left: false, bottom: true, right: false },
      cursor: 'ns-resize',
      style: { bottom: -half, left: s, right: s, height: s },
    },
    {
      edge: { top: false, left: true, bottom: false, right: false },
      cursor: 'ew-resize',
      style: { left: -half, top: s, bottom: s, width: s },
    },
    {
      edge: { top: false, left: false, bottom: false, right: true },
      cursor: 'ew-resize',
      style: { right: -half, top: s, bottom: s, width: s },
    },
  ]

  return (
    <>
      {edges.map((e, i) => (
        <div
          key={i}
          onPointerDown={() => onEdgePointerDown(e.edge)}
          style={{
            position: 'absolute',
            ...e.style,
            cursor: rotateCursor(e.cursor, rotationDeg),
            zIndex: 30,
          }}
        />
      ))}
    </>
  )
}
