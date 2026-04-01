'use client'

import { useEffect, useRef } from 'react'
import { useDrag } from '@use-gesture/react'
import {
  boundsToRegion,
  clampToDocument,
  expandBounds,
  withMargin,
  type Bounds,
} from '@/lib/features/canvas/stroke-session'
import {
  blobToUint8Array,
  convertToImageBitmap,
} from '@/lib/infra/media/assets'
import { useMaskCommands } from '@/hooks/documents/useMaskCommands'
import type {
  DocumentPointer,
  PointerToDocumentFn,
} from '@/hooks/canvas/usePointerToDocument'
import { usePreferencesState } from '@/hooks/ui/usePreferencesState'
import { useEditorUiState } from '@/hooks/ui/useEditorUiState'
import type { Document, InpaintRegion, ToolMode } from '@/types'

type MaskDrawingOptions = {
  mode: ToolMode
  currentDocument: Document | null
  pointerToDocument: PointerToDocumentFn
  showMask: boolean
  enabled: boolean
}

export function useMaskStrokeSession({
  mode,
  currentDocument,
  pointerToDocument,
  showMask,
  enabled,
}: MaskDrawingOptions) {
  const brushSize = usePreferencesState((state) => state.brushConfig.size)
  const { updateMask, inpaintPartial } = useMaskCommands()
  const currentDocumentId = useEditorUiState((state) => state.currentDocumentId)
  const canvasRef = useRef<HTMLCanvasElement | null>(null)
  const ctxRef = useRef<CanvasRenderingContext2D | null>(null)
  const drawingRef = useRef(false)
  const lastPointRef = useRef<DocumentPointer | null>(null)
  const boundsRef = useRef<Bounds | null>(null)
  const inpaintQueueRef = useRef<Promise<void>>(Promise.resolve())
  const isRepairMode = mode === 'repairBrush'
  const isEraseMode = mode === 'eraser'
  const isActive = enabled && (isRepairMode || isEraseMode)

  useEffect(() => {
    if (enabled) return
    drawingRef.current = false
    lastPointRef.current = null
    boundsRef.current = null
    inpaintQueueRef.current = Promise.resolve()
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
      if (ctx) {
        ctx.fillStyle = '#000'
        ctx.fillRect(0, 0, canvas.width, canvas.height)
      }
      ctx?.restore()
    }

    let cancelled = false
    const segment = currentDocument.segment
    if (segment && (showMask || isActive)) {
      void (async () => {
        try {
          const bitmap = await convertToImageBitmap(segment)
          if (cancelled) {
            bitmap.close()
            return
          }
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
      inpaintQueueRef.current = Promise.resolve()
    }
  }, [
    currentDocument?.id,
    currentDocument?.width,
    currentDocument?.height,
    currentDocument?.segment,
    isActive,
    showMask,
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

  const queueInpaint = (task: () => Promise<void>) => {
    inpaintQueueRef.current = inpaintQueueRef.current.catch(() => {}).then(task)
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
      queueInpaint(async () => {
        try {
          await inpaintPartial(region, {
            documentId: currentDocumentId,
          })
        } catch (error) {
          console.error(error)
        }
      })
    })()
  }

  const bind = useDrag(
    ({ first, last, event, active }) => {
      if (!isActive || !currentDocument) return
      const sourceEvent = event as MouseEvent
      const point = pointerToDocument(sourceEvent)
      if (!point) {
        if ((last || !active) && drawingRef.current) {
          finalizeStroke()
        }
        return
      }
      const clamped = clampToDocument(point, currentDocument)

      if (first) {
        drawingRef.current = true
        lastPointRef.current = clamped
        boundsRef.current = {
          minX: clamped.x - brushSize / 2,
          minY: clamped.y - brushSize / 2,
          maxX: clamped.x + brushSize / 2,
          maxY: clamped.y + brushSize / 2,
        }
        drawStroke(clamped, clamped)
        return
      }

      if (!drawingRef.current) return
      const lastPoint = lastPointRef.current ?? clamped
      drawStroke(lastPoint, clamped)
      lastPointRef.current = clamped
      boundsRef.current = boundsRef.current
        ? expandBounds(boundsRef.current, clamped, brushSize / 2)
        : {
            minX: clamped.x - brushSize / 2,
            minY: clamped.y - brushSize / 2,
            maxX: clamped.x + brushSize / 2,
            maxY: clamped.y + brushSize / 2,
          }

      if (last || !active) {
        finalizeStroke()
      }
    },
    {
      pointer: { buttons: 1, touch: true },
      preventDefault: true,
      filterTaps: true,
      eventOptions: { passive: false },
    },
  )

  return {
    canvasRef,
    visible: showMask,
    bind,
  }
}
