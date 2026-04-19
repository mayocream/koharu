'use client'

import { useRef } from 'react'

import { useCanvasDrawing, type CanvasDims } from '@/hooks/useCanvasDrawing'
import type { PointerToDocumentFn } from '@/hooks/usePointerToDocument'
import { getConfig, startPipeline } from '@/lib/api/default/default'
import type { Page } from '@/lib/api/schemas'
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
  segmentData?: Uint8Array
  pointerToDocument: PointerToDocumentFn
  showMask: boolean
  enabled: boolean
}

/**
 * Repair-brush canvas that edits the `Mask { role: segment }` node. On stroke
 * end:
 *   1. PUT the updated mask to `/api/v1/pages/{id}/masks/segment` (raw PNG).
 *   2. Kick a region-scoped inpainter via `POST /pipelines` so the inpainted
 *      layer refreshes just over the touched area.
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
    ? { width: page.width, height: page.height, key: page.id }
    : null

  const { canvasRef, bind: rawBind } = useCanvasDrawing(dims, pointerToDocument, {
    getColor: () => (isEraseMode ? '#000000' : '#ffffff'),
    blendMode: 'source-over',
    getBrushSize: () => usePreferencesStore.getState().brushConfig.size,
    enabled: showMask,
    onCanvasInit: (ctx, d) => {
      ctx.fillStyle = '#000'
      ctx.fillRect(0, 0, d.width, d.height)
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
      }
    },
    onFinalizeFullCanvas: async (fullPng) => {
      if (!page) return
      try {
        const res = await fetch(`/api/v1/pages/${page.id}/masks/segment`, {
          method: 'PUT',
          headers: { 'Content-Type': 'image/png' },
          body: fullPng as unknown as BodyInit,
        })
        if (!res.ok) throw new Error(`mask PUT failed: ${res.status}`)
      } catch (e) {
        useEditorUiStore.getState().showError(String(e))
      }
    },
    onFinalize: async (_patch, region) => {
      if (!page) return
      const brushSize = usePreferencesStore.getState().brushConfig.size
      const width = Math.max(brushSize, region.width)
      const margin = Math.min(width * 0.2, 32)
      const x0 = Math.max(0, Math.floor(region.x - margin))
      const y0 = Math.max(0, Math.floor(region.y - margin))
      const x1 = Math.min(page.width, Math.ceil(region.x + region.width + margin))
      const y1 = Math.min(page.height, Math.ceil(region.y + region.height + margin))
      const inpaintRegion = {
        x: x0,
        y: y0,
        width: Math.max(1, x1 - x0),
        height: Math.max(1, y1 - y0),
      }
      inpaintQueueRef.current = inpaintQueueRef.current
        .catch(() => {})
        .then(async () => {
          try {
            const cfg = await getConfig()
            const inpainter = cfg.pipeline?.inpainter || 'lama-manga'
            await startPipeline({
              steps: [inpainter],
              pages: [page.id],
              region: inpaintRegion,
            })
            useEditorUiStore.getState().setShowInpaintedImage(true)
          } catch (e) {
            useEditorUiStore.getState().showError(String(e))
          }
        })
    },
  })

  const bind = isActive ? rawBind : () => ({})
  return { canvasRef, visible: showMask, bind }
}
