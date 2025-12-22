'use client'

import type React from 'react'
import { useEffect, useRef } from 'react'
import { useAppStore, useConfigStore } from '@/lib/store'
import { blobToUint8Array, convertToImageBitmap } from '@/lib/util'
import { Document, InpaintRegion, ToolMode } from '@/types'
import {
  PointerToDocumentFn,
  type DocumentPointer,
} from '@/hooks/usePointerToDocument'

type MaskDrawingOptions = {
  mode: ToolMode
  currentDocument?: Document
  pointerToDocument: PointerToDocumentFn
  showMask: boolean
}

type Bounds = {
  minX: number
  minY: number
  maxX: number
  maxY: number
}

const clampToDocument = (
  point: DocumentPointer,
  doc?: Document,
): DocumentPointer => {
  if (!doc) return point
  return {
    x: Math.max(0, Math.min(doc.width, point.x)),
    y: Math.max(0, Math.min(doc.height, point.y)),
  }
}

const expandBounds = (bounds: Bounds, point: DocumentPointer, radius: number) =>
  ({
    minX: Math.min(bounds.minX, point.x - radius),
    minY: Math.min(bounds.minY, point.y - radius),
    maxX: Math.max(bounds.maxX, point.x + radius),
    maxY: Math.max(bounds.maxY, point.y + radius),
  }) satisfies Bounds

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
}: MaskDrawingOptions) {
  const {
    maskConfig: { brushSize, brushMode },
  } = useConfigStore()
  const updateMask = useAppStore((state) => state.updateMask)
  const inpaintPartial = useAppStore((state) => state.inpaintPartial)
  const currentDocumentIndex = useAppStore(
    (state) => state.currentDocumentIndex,
  )
  const canvasRef = useRef<HTMLCanvasElement | null>(null)
  const ctxRef = useRef<CanvasRenderingContext2D | null>(null)
  const drawingRef = useRef(false)
  const lastPointRef = useRef<DocumentPointer | null>(null)
  const boundsRef = useRef<Bounds | null>(null)
  const inpaintQueueRef = useRef<Promise<void>>(Promise.resolve())

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

    canvas.width = currentDocument.width
    canvas.height = currentDocument.height
    ctx?.clearRect(0, 0, canvas.width, canvas.height)
    ctx?.save()
    ctx && (ctx.fillStyle = '#000')
    ctx?.fillRect(0, 0, canvas.width, canvas.height)
    ctx?.restore()

    let cancelled = false
    if (currentDocument.segment) {
      void (async () => {
        try {
          const bitmap = await convertToImageBitmap(currentDocument.segment!)
          if (cancelled) {
            bitmap.close()
            return
          }
          ctx?.clearRect(0, 0, canvas.width, canvas.height)
          ctx?.drawImage(
            bitmap,
            0,
            0,
            currentDocument.width,
            currentDocument.height,
          )
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
      inpaintQueueRef.current = Promise.resolve()
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
    ctx.strokeStyle = brushMode === 'eraser' ? '#000000' : '#ffffff'
    ctx.fillStyle = ctx.strokeStyle
    ctx.globalCompositeOperation = 'source-over'

    ctx.beginPath()
    ctx.moveTo(from.x, from.y)
    ctx.lineTo(to.x, to.y)
    ctx.stroke()
    ctx.restore()
  }

  const exportMaskBytes = async (): Promise<number[] | null> => {
    const canvas = canvasRef.current
    if (!canvas) return null
    const blob = await new Promise<Blob | null>((resolve) => {
      canvas.toBlob((result) => resolve(result), 'image/png')
    })
    if (!blob) return null
    return blobToUint8Array(blob)
  }

  const queueInpaint = (task: () => Promise<void>) => {
    inpaintQueueRef.current = inpaintQueueRef.current.catch(() => {}).then(task)
  }

  const finalizeStroke = () => {
    if (!currentDocument || !boundsRef.current) return
    const region = withMargin(boundsRef.current, brushSize, currentDocument)
    boundsRef.current = null
    drawingRef.current = false
    lastPointRef.current = null

    void (async () => {
      const maskBytes = await exportMaskBytes()
      if (!maskBytes) return
      try {
        await updateMask(maskBytes, { sync: false })
      } catch (error) {
        console.error(error)
      }
      queueInpaint(async () => {
        try {
          await inpaintPartial(region, {
            mask: maskBytes,
            index: currentDocumentIndex,
          })
        } catch (error) {
          console.error(error)
          void updateMask(maskBytes, { sync: true })
        }
      })
    })()
  }

  const handlePointerDown = (event: React.PointerEvent<HTMLCanvasElement>) => {
    if (mode !== 'mask' || !currentDocument) return
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
    if (!drawingRef.current || mode !== 'mask' || !currentDocument) return
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
    visible: showMask || mode === 'mask',
    handlePointerDown,
    handlePointerMove,
    handlePointerUp,
    handlePointerLeave,
  }
}
