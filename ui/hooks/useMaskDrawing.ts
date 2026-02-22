'use client'

import type React from 'react'
import { useEffect, useRef } from 'react'
import { useAppStore, useConfigStore } from '@/lib/store'
import { blobToUint8Array, convertToImageBitmap } from '@/lib/util'
import type { Document, InpaintRegion, ToolMode } from '@/types'
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
  selectInpaintPartial,
  selectUpdateMask,
} from '@/lib/store-selectors'

type MaskDrawingOptions = {
  mode: ToolMode
  currentDocument: Document | null
  pointerToDocument: PointerToDocumentFn
  showMask: boolean
  enabled: boolean
}

const withMargin = (
  bounds: Bounds,
  brushSize: number,
  doc: Document,
): InpaintRegion => {
  const width = Math.max(brushSize, bounds.maxX - bounds.minX)
  const height = Math.max(brushSize, bounds.maxY - bounds.minY)
  const margin = Math.min(width * 0.2, 32)

  const x0 = Math.max(0, Math.floor(bounds.minX - margin))
  const y0 = Math.max(0, Math.floor(bounds.minY - margin))
  const x1 = Math.min(doc.width, Math.ceil(bounds.maxX + margin))
  const y1 = Math.min(doc.height, Math.ceil(bounds.maxY + margin))

  return {
    x: x0,
    y: y0,
    width: Math.max(1, x1 - x0),
    height: Math.max(1, y1 - y0),
  }
}

export function useMaskDrawing({
  mode,
  currentDocument,
  pointerToDocument,
  showMask,
  enabled,
}: MaskDrawingOptions) {
  const brushSize = useConfigStore(selectBrushConfig).size
  const updateMask = useAppStore(selectUpdateMask)
  const inpaintPartial = useAppStore(selectInpaintPartial)
  const currentDocumentIndex = useAppStore(selectCurrentDocumentIndex)
  const canvasRef = useRef<HTMLCanvasElement | null>(null)
  const ctxRef = useRef<CanvasRenderingContext2D | null>(null)
  const drawingRef = useRef(false)
  const lastPointRef = useRef<DocumentPointer | null>(null)
  const boundsRef = useRef<Bounds | null>(null)
  const inpaintQueueRef = useRef(createSerialQueue())
  const isRepairMode = mode === 'repairBrush'
  const isEraseMode = mode === 'eraser'
  const isActive = enabled && (isRepairMode || isEraseMode)

  // Reset drawing state when interaction is disabled so stale strokes don't carry over.
  useEffect(() => {
    if (enabled) return
    drawingRef.current = false
    lastPointRef.current = null
    boundsRef.current = null
    inpaintQueueRef.current.reset()
  }, [enabled, mode])

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    const ctx = canvas.getContext('2d')
    ctxRef.current = ctx

    if (!currentDocument) {
      canvas.width = 0
      canvas.height = 0
      ctx?.clearRect(0, 0, canvas.width, canvas.height)
      return
    }

    const needsResize =
      canvas.width !== currentDocument.width ||
      canvas.height !== currentDocument.height

    if (needsResize) {
      canvas.width = currentDocument.width
      canvas.height = currentDocument.height
      ctx?.clearRect(0, 0, canvas.width, canvas.height)
      ctx?.save()
      ctx && (ctx.fillStyle = '#000')
      ctx?.fillRect(0, 0, canvas.width, canvas.height)
      ctx?.restore()
    }

    let cancelled = false
    if (currentDocument.segment) {
      void (async () => {
        try {
          const bitmap = await convertToImageBitmap(currentDocument.segment!)
          if (cancelled) {
            bitmap.close()
            return
          }
          // Redraw atomically once the new bitmap is ready so the previous
          // mask stays visible until swap, avoiding a flicker.
          ctx?.save()
          ctx?.clearRect(0, 0, canvas.width, canvas.height)
          ctx?.drawImage(
            bitmap,
            0,
            0,
            currentDocument.width,
            currentDocument.height,
          )
          ctx?.restore()
          bitmap.close()
        } catch (error) {
          console.error(error)
        }
      })()
    }

    return () => {
      cancelled = true
      drawingRef.current = false
      lastPointRef.current = null
      boundsRef.current = null
      inpaintQueueRef.current.reset()
    }
  }, [
    currentDocument?.id,
    currentDocument?.width,
    currentDocument?.height,
    currentDocument?.segment,
  ])

  const drawStroke = (from: DocumentPointer, to: DocumentPointer) => {
    const ctx = ctxRef.current
    if (!ctx) return
    ctx.save()
    ctx.lineCap = 'round'
    ctx.lineJoin = 'round'
    ctx.lineWidth = brushSize
    ctx.strokeStyle = isEraseMode ? '#000000' : '#ffffff'
    ctx.fillStyle = ctx.strokeStyle
    ctx.globalCompositeOperation = 'source-over'

    ctx.beginPath()
    ctx.moveTo(from.x, from.y)
    ctx.lineTo(to.x, to.y)
    ctx.stroke()
    ctx.restore()
  }

  const exportMaskBytes = async (): Promise<Uint8Array | null> => {
    const canvas = canvasRef.current
    if (!canvas) return null
    const blob = await new Promise<Blob | null>((resolve) => {
      canvas.toBlob((result) => resolve(result), 'image/png')
    })
    if (!blob) return null
    return blobToUint8Array(blob)
  }

  const exportMaskPatch = async (
    region: InpaintRegion,
  ): Promise<Uint8Array | null> => {
    const canvas = canvasRef.current
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
    if (!isActive) return
    const strokeBounds = boundsRef.current
    if (!currentDocument || !strokeBounds) return
    const patchRegion = boundsToRegion(strokeBounds, currentDocument)
    const region = withMargin(strokeBounds, brushSize, currentDocument)
    boundsRef.current = null
    drawingRef.current = false
    lastPointRef.current = null

    void (async () => {
      const [maskBytes, patchBytes] = await Promise.all([
        exportMaskBytes(),
        exportMaskPatch(patchRegion),
      ])
      if (!maskBytes) return
      try {
        await updateMask(maskBytes, {
          patchRegion: patchBytes ? patchRegion : undefined,
          patch: patchBytes ?? undefined,
        })
      } catch (error) {
        console.error(error)
      }
      void inpaintQueueRef.current.push(async () => {
        try {
          await inpaintPartial(region, {
            index: currentDocumentIndex,
          })
        } catch (error) {
          console.error(error)
        }
      })
    })()
  }

  const handlePointerDown = (event: React.PointerEvent<HTMLCanvasElement>) => {
    if (!isActive || !currentDocument) return
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
    if (!drawingRef.current || !isActive || !currentDocument) return
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
    if (!drawingRef.current) return
    event.stopPropagation()
    finalizeStroke()
  }

  const handlePointerLeave = () => {
    if (!drawingRef.current) return
    finalizeStroke()
  }

  return {
    canvasRef,
    visible: showMask,
    handlePointerDown,
    handlePointerMove,
    handlePointerUp,
    handlePointerLeave,
  }
}
