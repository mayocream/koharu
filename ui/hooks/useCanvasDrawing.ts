'use client'

import { useDrag } from '@use-gesture/react'
import { useEffect, useRef, type RefObject } from 'react'

import { type PointerToDocumentFn, type DocumentPointer } from '@/hooks/usePointerToDocument'
import type { MappedDocument } from '@/hooks/useTextBlocks'
import { blobToUint8Array } from '@/lib/util'
import type { InpaintRegion } from '@/types'

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

type Bounds = {
  minX: number
  minY: number
  maxX: number
  maxY: number
}

export type CanvasDrawingConfig = {
  getColor: () => string
  blendMode: GlobalCompositeOperation
  getBrushSize: () => number
  onFinalize: (patch: Uint8Array, region: InpaintRegion) => Promise<void>
  /** Called after finalize with the full-canvas PNG and the patch region. */
  onFinalizeFullCanvas?: (fullPng: Uint8Array, patchRegion: InpaintRegion) => Promise<void>
  enabled: boolean
  /** Optional second canvas to mirror strokes to. */
  targetCanvasRef?: RefObject<HTMLCanvasElement | null>
  /** When true, clear the drawing canvas after each stroke finalize. */
  clearAfterStroke?: boolean
  /** Called to set up the canvas content when the document changes (e.g. draw existing mask). */
  onCanvasInit?: (ctx: CanvasRenderingContext2D, doc: MappedDocument) => void | Promise<void>
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const clampToDocument = (point: DocumentPointer, doc?: MappedDocument): DocumentPointer => {
  if (!doc) return point
  return {
    x: Math.max(0, Math.min(doc.width, point.x)),
    y: Math.max(0, Math.min(doc.height, point.y)),
  }
}

const expandBounds = (bounds: Bounds, point: DocumentPointer, radius: number): Bounds => ({
  minX: Math.min(bounds.minX, point.x - radius),
  minY: Math.min(bounds.minY, point.y - radius),
  maxX: Math.max(bounds.maxX, point.x + radius),
  maxY: Math.max(bounds.maxY, point.y + radius),
})

const boundsToRegion = (bounds: Bounds, doc: MappedDocument): InpaintRegion => {
  const x0 = Math.max(0, Math.floor(bounds.minX))
  const y0 = Math.max(0, Math.floor(bounds.minY))
  const x1 = Math.min(doc.width, Math.ceil(bounds.maxX))
  const y1 = Math.min(doc.height, Math.ceil(bounds.maxY))
  return {
    x: x0,
    y: y0,
    width: Math.max(1, x1 - x0),
    height: Math.max(1, y1 - y0),
  }
}

const exportCanvasRegion = async (
  canvas: HTMLCanvasElement,
  region: InpaintRegion,
): Promise<Uint8Array | null> => {
  if (region.width <= 0 || region.height <= 0) return null
  const tmp = document.createElement('canvas')
  tmp.width = region.width
  tmp.height = region.height
  const ctx = tmp.getContext('2d')
  if (!ctx) return null
  ctx.drawImage(
    canvas,
    region.x,
    region.y,
    region.width,
    region.height,
    0,
    0,
    region.width,
    region.height,
  )
  const blob = await new Promise<Blob | null>((r) => tmp.toBlob(r, 'image/png'))
  return blob ? blobToUint8Array(blob) : null
}

const exportFullCanvas = async (canvas: HTMLCanvasElement): Promise<Uint8Array | null> => {
  const blob = await new Promise<Blob | null>((r) => canvas.toBlob(r, 'image/png'))
  return blob ? blobToUint8Array(blob) : null
}

const initBounds = (point: DocumentPointer, radius: number): Bounds => ({
  minX: point.x - radius,
  minY: point.y - radius,
  maxX: point.x + radius,
  maxY: point.y + radius,
})

// ---------------------------------------------------------------------------
// Hook
// ---------------------------------------------------------------------------

export function useCanvasDrawing(
  currentDocument: MappedDocument | null,
  pointerToDocument: PointerToDocumentFn,
  config: CanvasDrawingConfig,
) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null)
  const ctxRef = useRef<CanvasRenderingContext2D | null>(null)
  const drawingRef = useRef(false)
  const lastPointRef = useRef<DocumentPointer | null>(null)
  const boundsRef = useRef<Bounds | null>(null)

  // Reset drawing state when disabled
  useEffect(() => {
    if (config.enabled) return
    drawingRef.current = false
    lastPointRef.current = null
    boundsRef.current = null
  }, [config.enabled])

  // Canvas setup and resize
  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    const ctx = canvas.getContext('2d')
    ctxRef.current = ctx

    if (!currentDocument || !config.enabled) {
      canvas.width = 0
      canvas.height = 0
      ctx?.clearRect(0, 0, canvas.width, canvas.height)
      return () => {
        drawingRef.current = false
        lastPointRef.current = null
        boundsRef.current = null
      }
    }

    const needsResize =
      canvas.width !== currentDocument.width || canvas.height !== currentDocument.height

    if (needsResize) {
      canvas.width = currentDocument.width
      canvas.height = currentDocument.height
    }
    ctx?.clearRect(0, 0, canvas.width, canvas.height)

    if (config.onCanvasInit && ctx) {
      const result = config.onCanvasInit(ctx, currentDocument)
      if (result && typeof (result as any).then === 'function') {
        void (result as Promise<void>).catch(console.error)
      }
    }

    return () => {
      drawingRef.current = false
      lastPointRef.current = null
      boundsRef.current = null
    }
  }, [currentDocument?.id, currentDocument?.width, currentDocument?.height, config.enabled])

  const drawStroke = (from: DocumentPointer, to: DocumentPointer) => {
    const color = config.getColor()
    const brushSize = config.getBrushSize()

    const stroke = (ctx: CanvasRenderingContext2D) => {
      ctx.save()
      ctx.lineCap = 'round'
      ctx.lineJoin = 'round'
      ctx.lineWidth = brushSize
      ctx.strokeStyle = color
      ctx.fillStyle = color
      ctx.globalCompositeOperation = config.blendMode
      ctx.beginPath()
      ctx.moveTo(from.x, from.y)
      ctx.lineTo(to.x, to.y)
      ctx.stroke()
      ctx.restore()
    }

    const ctx = ctxRef.current
    if (ctx) stroke(ctx)

    const targetCtx = config.targetCanvasRef?.current?.getContext('2d')
    if (targetCtx) stroke(targetCtx)
  }

  const finalizeStroke = () => {
    if (!config.enabled) return
    const strokeBounds = boundsRef.current
    if (!currentDocument || !strokeBounds) return
    const patchRegion = boundsToRegion(strokeBounds, currentDocument)
    boundsRef.current = null
    drawingRef.current = false
    lastPointRef.current = null

    void (async () => {
      const sourceCanvas = config.targetCanvasRef?.current ?? canvasRef.current
      if (!sourceCanvas) return
      const patchBytes = await exportCanvasRegion(sourceCanvas, patchRegion)

      if (config.onFinalizeFullCanvas) {
        const fullBytes = await exportFullCanvas(canvasRef.current!)
        if (fullBytes) {
          try {
            await config.onFinalizeFullCanvas(fullBytes, patchRegion)
          } catch (e) {
            console.error(e)
          }
        }
      }

      if (patchBytes) {
        try {
          await config.onFinalize(patchBytes, patchRegion)
        } catch (e) {
          console.error(e)
        }
      }

      if (config.clearAfterStroke) {
        const ctx = ctxRef.current
        const canvas = canvasRef.current
        if (ctx && canvas) ctx.clearRect(0, 0, canvas.width, canvas.height)
      }
    })()
  }

  const bind = useDrag(
    ({ first, last, event, active }) => {
      if (!config.enabled || !currentDocument) return
      const sourceEvent = event as MouseEvent
      const point = pointerToDocument(sourceEvent)
      if (!point) {
        if ((last || !active) && drawingRef.current) finalizeStroke()
        return
      }
      const clamped = clampToDocument(point, currentDocument)
      const brushSize = config.getBrushSize()
      const radius = brushSize / 2

      if (first) {
        drawingRef.current = true
        lastPointRef.current = clamped
        boundsRef.current = initBounds(clamped, radius)
        drawStroke(clamped, clamped)
        return
      }

      if (!drawingRef.current) return
      const lastPoint = lastPointRef.current ?? clamped
      drawStroke(lastPoint, clamped)
      lastPointRef.current = clamped
      boundsRef.current = boundsRef.current
        ? expandBounds(boundsRef.current, clamped, radius)
        : initBounds(clamped, radius)

      if (last || !active) finalizeStroke()
    },
    {
      pointer: { buttons: 1, touch: true },
      preventDefault: true,
      filterTaps: true,
      eventOptions: { passive: false },
    },
  )

  return { canvasRef, visible: config.enabled, bind }
}
