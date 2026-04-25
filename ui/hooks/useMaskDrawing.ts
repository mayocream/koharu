'use client'

import { useEffect, useRef } from 'react'

import { useCanvasDrawing, type CanvasDims } from '@/hooks/useCanvasDrawing'
import type { PointerToDocumentFn } from '@/hooks/usePointerToDocument'
import { getConfig } from '@/lib/api/default/default'
import type { Page } from '@/lib/api/schemas'
import { invalidateScene } from '@/lib/io/scene'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'
import type { ToolMode } from '@/lib/types'

async function convertBytesToBitmap(bytes: Uint8Array): Promise<ImageBitmap> {
  const blob = new Blob([bytes as unknown as BlobPart])
  return createImageBitmap(blob)
}

type MaskDrawingOptions = {
  mode: ToolMode
  page: Page | null
  maskHash?: string | null
  segmentData?: Uint8Array
  pointerToDocument: PointerToDocumentFn
  showMask: boolean
  enabled: boolean
}

/**
 * Repair-brush canvas that edits the `Mask { role: segment }` node. On stroke
 * end, it performs an atomic update:
 *   1. PUT the updated mask to `/api/v1/pages/{id}/masks/segment` (raw PNG).
 *   2. Includes inpainter pipeline and region parameters in the query string
 *      to trigger the AI result in the same backend transaction.
 */
export function useMaskDrawing({
  mode,
  page,
  segmentData,
  pointerToDocument,
  showMask,
  enabled,
}: MaskDrawingOptions) {
  const inpaintQueueRef = useRef<Promise<void>>(Promise.resolve())
  const isEraseMode = mode === 'eraser'
  const isActive = enabled && (mode === 'repairBrush' || isEraseMode)

  const dims: CanvasDims | null = page
    ? {
        width: page.width,
        height: page.height,
        key: page.id,
      }
    : null

  const { canvasRef, bind: rawBind } = useCanvasDrawing(dims, pointerToDocument, {
    getColor: () => (isEraseMode ? '#000000' : '#ffffff'),
    blendMode: 'source-over',
    getBrushSize: () => usePreferencesStore.getState().brushConfig.size,
    enabled: showMask,
    onCanvasInit: (ctx, d) => {
      if (segmentData) {
        void (async () => {
          try {
            const bitmap = await convertBytesToBitmap(segmentData)
            ctx.save()
            ctx.clearRect(0, 0, d.width, d.height)
            ctx.drawImage(bitmap, 0, 0, d.width, d.height)
            ctx.restore()
            bitmap.close()
          } catch (e) {
            console.error(e)
          }
        })()
      } else {
        ctx.fillStyle = '#000'
        ctx.fillRect(0, 0, d.width, d.height)
      }
    },
    onFinalizeFullCanvas: async (fullPng, region) => {
      if (!page) return

      // Chain the request to prevent concurrent ML runs and race conditions
      inpaintQueueRef.current = inpaintQueueRef.current.then(async () => {
        try {
          const config = await getConfig()
          const inpainter = config.pipeline?.inpainter || 'lama-manga'

          const params = new URLSearchParams({
            pipeline: inpainter,
            x: region.x.toString(),
            y: region.y.toString(),
            width: region.width.toString(),
            height: region.height.toString(),
          })

          const res = await fetch(`/api/v1/pages/${page.id}/masks/segment?${params}`, {
            method: 'PUT',
            headers: { 'Content-Type': 'image/png' },
            body: fullPng as unknown as BodyInit,
          })
          if (!res.ok) throw new Error(`mask PUT failed: ${res.status}`)
          await invalidateScene()
          useEditorUiStore.getState().setShowInpaintedImage(true)
        } catch (e) {
          useEditorUiStore.getState().showError(String(e))
        }
      })

      await inpaintQueueRef.current
    },
    onFinalize: async () => {},
  })
  // Watch for mask data changes and repaint. This handles the case where
  // segmentData updates after the canvas key change / onCanvasInit.
  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas || !segmentData || !page) return
    const ctx = canvas.getContext('2d')
    if (!ctx) return

    let cancelled = false
    void (async () => {
      try {
        const bitmap = await convertBytesToBitmap(segmentData)
        if (cancelled) {
          bitmap.close()
          return
        }
        ctx.save()
        ctx.clearRect(0, 0, page.width, page.height)
        ctx.drawImage(bitmap, 0, 0, page.width, page.height)
        ctx.restore()
        bitmap.close()
      } catch (e) {
        console.error('Failed to repaint mask:', e)
      }
    })()
    return () => {
      cancelled = true
    }
  }, [segmentData, page?.id, page?.width, page?.height, canvasRef, showMask])

  const bind = isActive ? rawBind : () => ({})
  return { canvasRef, visible: showMask, bind }
}
