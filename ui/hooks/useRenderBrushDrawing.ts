'use client'

import { useEffect, useRef, type RefObject } from 'react'
import { useAppStore, useConfigStore } from '@/lib/store'
import type { Document, InpaintRegion, ToolMode } from '@/types'
import { blobToUint8Array } from '@/lib/util'
import {
  PointerToDocumentFn,
  type DocumentPointer,
} from '@/hooks/usePointerToDocument'
import {
  boundsToRegion,
  clampToDocument,
  expandBounds,
  type Bounds,
} from '@/hooks/drawing/geometry'
import { createSerialQueue } from '@/hooks/drawing/serialQueue'
import {
  selectBrushConfig,
  selectCurrentDocumentIndex,
  selectPaintRendered,
} from '@/lib/store-selectors'

type RenderBrushOptions = {
  mode: ToolMode
  currentDocument: Document | null
  pointerToDocument: PointerToDocumentFn
  enabled: boolean
  action: 'paint' | 'erase'
  targetCanvasRef?: RefObject<HTMLCanvasElement | null>
}

export function useRenderBrushDrawing({
  mode,
  currentDocument,
  pointerToDocument,
  enabled,
  action,
  targetCanvasRef,
}: RenderBrushOptions) {
  const { size: brushSize, color: brushColor } =
    useConfigStore(selectBrushConfig)
  const paintRendered = useAppStore(selectPaintRendered)
  const currentDocumentIndex = useAppStore(selectCurrentDocumentIndex)
  const canvasRef = useRef<HTMLCanvasElement | null>(null)
  const ctxRef = useRef<CanvasRenderingContext2D | null>(null)
  const drawingRef = useRef(false)
  const lastPointRef = useRef<DocumentPointer | null>(null)
  const boundsRef = useRef<Bounds | null>(null)
  const paintQueueRef = useRef(createSerialQueue())

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    const ctx = canvas.getContext('2d')
    ctxRef.current = ctx

    if (!currentDocument || !enabled) {
      canvas.width = 0
      canvas.height = 0
      ctx?.clearRect(0, 0, canvas.width, canvas.height)
      return () => {
        drawingRef.current = false
        lastPointRef.current = null
        boundsRef.current = null
        paintQueueRef.current.reset()
      }
    }

    const needsResize =
      canvas.width !== currentDocument.width ||
      canvas.height !== currentDocument.height

    if (needsResize) {
      canvas.width = currentDocument.width
      canvas.height = currentDocument.height
    }
    ctx?.clearRect(0, 0, canvas.width, canvas.height)

    return () => {
      drawingRef.current = false
      lastPointRef.current = null
      boundsRef.current = null
      paintQueueRef.current.reset()
    }
  }, [
    currentDocument?.id,
    currentDocument?.width,
    currentDocument?.height,
    mode,
    enabled,
  ])

  const drawStroke = (from: DocumentPointer, to: DocumentPointer) => {
    if (!enabled) return
    const isErasing = action === 'erase'
    const stroke = (ctx: CanvasRenderingContext2D) => {
      ctx.save()
      ctx.lineCap = 'round'
      ctx.lineJoin = 'round'
      ctx.lineWidth = brushSize
      ctx.strokeStyle = isErasing ? '#000000' : brushColor
      ctx.fillStyle = ctx.strokeStyle
      ctx.globalCompositeOperation = isErasing
        ? 'destination-out'
        : 'source-over'
      ctx.beginPath()
      ctx.moveTo(from.x, from.y)
      ctx.lineTo(to.x, to.y)
      ctx.stroke()
      ctx.restore()
    }

    const ctx = ctxRef.current
    if (ctx) stroke(ctx)

    const targetCanvas = targetCanvasRef?.current
    const targetCtx = targetCanvas?.getContext('2d')
    if (targetCtx) stroke(targetCtx)
  }

  const exportPatch = async (
    region: InpaintRegion,
  ): Promise<Uint8Array | null> => {
    const canvas = targetCanvasRef?.current ?? canvasRef.current
    if (!canvas || region.width <= 0 || region.height <= 0) return null

    const tempCanvas = document.createElement('canvas')
    tempCanvas.width = region.width
    tempCanvas.height = region.height
    const tempCtx = tempCanvas.getContext('2d')
    if (!tempCtx) return null

    tempCtx.drawImage(
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

    const blob = await new Promise<Blob | null>((resolve) => {
      tempCanvas.toBlob((result) => resolve(result), 'image/png')
    })
    if (!blob) return null
    return blobToUint8Array(blob)
  }

  const finalizeStroke = () => {
    if (!enabled) return
    const strokeBounds = boundsRef.current
    if (!currentDocument || !strokeBounds) return
    const patchRegion = boundsToRegion(strokeBounds, currentDocument)
    boundsRef.current = null
    drawingRef.current = false
    lastPointRef.current = null

    void (async () => {
      const patchBytes = await exportPatch(patchRegion)
      if (!patchBytes) {
        const canvas = canvasRef.current
        const ctx = ctxRef.current
        if (canvas && ctx) {
          ctx.clearRect(0, 0, canvas.width, canvas.height)
        }
        return
      }
      void paintQueueRef.current.push(async () => {
        try {
          await paintRendered(patchBytes, patchRegion, {
            index: currentDocumentIndex,
          })
        } catch (error) {
          console.error(error)
        }
        const canvas = canvasRef.current
        const ctx = ctxRef.current
        if (canvas && ctx) {
          ctx.clearRect(0, 0, canvas.width, canvas.height)
        }
      })
    })()
  }

  const handlePointerDown = (event: React.PointerEvent<HTMLCanvasElement>) => {
    if (!enabled || !currentDocument) return
    if (event.button !== 0) return
    const point = pointerToDocument(event)
    if (!point) return
    const clamped = clampToDocument(point, currentDocument)
    event.preventDefault()
    event.stopPropagation()
    drawingRef.current = true
    lastPointRef.current = clamped
    boundsRef.current = {
      minX: clamped.x - brushSize / 2,
      minY: clamped.y - brushSize / 2,
      maxX: clamped.x + brushSize / 2,
      maxY: clamped.y + brushSize / 2,
    }
    drawStroke(clamped, clamped)
  }

  const handlePointerMove = (event: React.PointerEvent<HTMLCanvasElement>) => {
    if (!enabled || !drawingRef.current || !currentDocument) return
    const point = pointerToDocument(event)
    if (!point) return
    const clamped = clampToDocument(point, currentDocument)
    event.stopPropagation()
    const last = lastPointRef.current ?? clamped
    drawStroke(last, clamped)
    lastPointRef.current = clamped
    boundsRef.current = boundsRef.current
      ? expandBounds(boundsRef.current, clamped, brushSize / 2)
      : {
          minX: clamped.x - brushSize / 2,
          minY: clamped.y - brushSize / 2,
          maxX: clamped.x + brushSize / 2,
          maxY: clamped.y + brushSize / 2,
        }
  }

  const handlePointerUp = (event: React.PointerEvent<HTMLCanvasElement>) => {
    if (!drawingRef.current || !enabled) return
    event.stopPropagation()
    finalizeStroke()
  }

  const handlePointerLeave = () => {
    if (!drawingRef.current || !enabled) return
    finalizeStroke()
  }

  return {
    canvasRef,
    visible: enabled,
    handlePointerDown,
    handlePointerMove,
    handlePointerUp,
    handlePointerLeave,
  }
}
