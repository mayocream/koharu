'use client'

import { useDrag } from '@use-gesture/react'
import { useMemo, useRef } from 'react'
import { useHotkeys } from 'react-hotkeys-hook'

import { useBlobImage } from '@/hooks/useBlobData'
import { useCurrentPage, useTextNodes, type TextNodeEntry } from '@/hooks/useCurrentPage'
import type { Transform } from '@/lib/api/schemas'
import { applyOp } from '@/lib/io/scene'
import { ops } from '@/lib/ops'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { useSelectionStore } from '@/lib/stores/selectionStore'

type TextBlockLayerProps = {
  showSprites?: boolean
  scale: number
  style?: React.CSSProperties
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

  const firstSelectedId = useMemo(() => {
    for (const id of selectedIds) if (id) return id
    return null
  }, [selectedIds])

  const removeNode = async (id: string) => {
    if (!page) return
    const node = page.nodes[id]
    if (!node) return
    const idx = Object.keys(page.nodes).indexOf(id)
    await applyOp(ops.removeNode(page.id, id, node, idx < 0 ? 0 : idx))
  }

  const updateTransform = async (id: string, t: Transform) => {
    if (!page) return
    await applyOp(ops.updateNode(page.id, id, { transform: t }))
  }

  useHotkeys(
    'delete',
    () => {
      if (firstSelectedId && interactive) void removeNode(firstSelectedId)
    },
    { enabled: !!firstSelectedId && interactive },
    [firstSelectedId, interactive],
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
          onSelect={(id) => select(id, false)}
          onCommit={(t) => void updateTransform(n.id, t)}
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
  onSelect: (id: string) => void
  onCommit: (transform: Transform) => void
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

  const setBox = (x: number, y: number, w: number, h: number) => {
    const el = boxRef.current
    if (!el) return
    el.style.transform = `translate(${x}px, ${y}px)`
    el.style.width = `${w}px`
    el.style.height = `${h}px`
  }

  const t = node.transform

  const bind = useDrag(
    ({ first, last, movement: [mx, my], event, tap }) => {
      if (!interactive) return
      event?.stopPropagation()
      if (tap) {
        onSelect(node.id)
        return
      }
      if (first) {
        dragStart.current = {
          x: t.x * scale,
          y: t.y * scale,
          w: t.width * scale,
          h: t.height * scale,
        }
        onSelect(node.id)
      }
      const { x: sx, y: sy, w: sw, h: sh } = dragStart.current
      const edge = edgeRef.current
      if (isResizeRef.current && edge) {
        let dx = 0
        let dy = 0
        let w = sw
        let h = sh
        if (edge.right) w += mx
        if (edge.left) {
          w -= mx
          dx = mx
        }
        if (edge.bottom) h += my
        if (edge.top) {
          h -= my
          dy = my
        }
        w = Math.max(4 * scale, w)
        h = Math.max(4 * scale, h)
        if (edge.left && w === 4 * scale) dx = sw - 4 * scale
        if (edge.top && h === 4 * scale) dy = sh - 4 * scale
        setBox(sx + dx, sy + dy, w, h)
        if (last) {
          isResizeRef.current = false
          edgeRef.current = null
          onCommit({
            x: Math.round((sx + dx) / scale),
            y: Math.round((sy + dy) / scale),
            width: Math.max(4, Math.round(w / scale)),
            height: Math.max(4, Math.round(h / scale)),
            rotationDeg: t.rotationDeg ?? 0,
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
            rotationDeg: t.rotationDeg ?? 0,
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
      style={{
        position: 'absolute',
        top: 0,
        left: 0,
        transform: `translate(${t.x * scale}px, ${t.y * scale}px)`,
        width: w,
        height: h,
        pointerEvents: interactive ? 'auto' : 'none',
        zIndex: selected ? 20 : 10,
        touchAction: 'none',
        cursor: interactive ? 'move' : 'default',
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
      >
        {index + 1}
      </div>
      {selected && interactive && <ResizeHandles onEdgePointerDown={handleEdgePointerDown} />}
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
        transform: `translate(${x}px, ${y}px) scale(${scale})`,
      }}
    />
  )
}

function ResizeHandles({ onEdgePointerDown }: { onEdgePointerDown: (edge: ResizeEdge) => void }) {
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
          style={{ position: 'absolute', ...e.style, cursor: e.cursor, zIndex: 30 }}
        />
      ))}
    </>
  )
}
