'use client'

import { useCallback, useRef } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'
import {
  updateMask as updateMaskApi,
  inpaintRegion as inpaintRegionApi,
} from '@/lib/api/regions/regions'
import {
  getGetDocumentQueryKey,
  getListDocumentsQueryKey,
} from '@/lib/api/documents/documents'
import { convertToImageBitmap } from '@/lib/util'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { normalizeErrorMessage } from '@/lib/errors'
import type { ToolMode } from '@/types'
import type { MappedDocument } from '@/hooks/useTextBlocks'
import type { PointerToDocumentFn } from '@/hooks/usePointerToDocument'
import { useCanvasDrawing } from '@/hooks/useCanvasDrawing'

type MaskDrawingOptions = {
  mode: ToolMode
  currentDocument: MappedDocument | null
  segmentData?: Uint8Array
  pointerToDocument: PointerToDocumentFn
  showMask: boolean
  enabled: boolean
}

export function useMaskDrawing({
  mode,
  currentDocument,
  segmentData,
  pointerToDocument,
  showMask,
  enabled,
}: MaskDrawingOptions) {
  const queryClient = useQueryClient()
  const inpaintQueueRef = useRef<Promise<void>>(Promise.resolve())
  const isEraseMode = mode === 'eraser'
  const isActive = enabled && (mode === 'repairBrush' || isEraseMode)

  const invalidateDocument = useCallback(
    async (documentId: string) => {
      await queryClient.invalidateQueries({
        queryKey: getGetDocumentQueryKey(documentId),
      })
      await queryClient.invalidateQueries({
        queryKey: getListDocumentsQueryKey(),
      })
    },
    [queryClient],
  )

  const { canvasRef, bind: rawBind } = useCanvasDrawing(
    currentDocument,
    pointerToDocument,
    {
      getColor: () => (isEraseMode ? '#000000' : '#ffffff'),
      blendMode: 'source-over',
      getBrushSize: () => usePreferencesStore.getState().brushConfig.size,
      enabled: showMask,
      onCanvasInit: (ctx, doc) => {
        // Fill black then draw existing segment mask on top
        ctx.fillStyle = '#000'
        ctx.fillRect(0, 0, doc.width, doc.height)
        if (segmentData) {
          void (async () => {
            try {
              const bitmap = await convertToImageBitmap(segmentData)
              ctx.save()
              ctx.clearRect(0, 0, doc.width, doc.height)
              ctx.drawImage(bitmap, 0, 0, doc.width, doc.height)
              ctx.restore()
              bitmap.close()
            } catch (e) {
              console.error(e)
            }
          })()
        }
      },
      onFinalizeFullCanvas: async (fullPng) => {
        const documentId = useEditorUiStore.getState().currentDocumentId
        if (!documentId) return
        try {
          await updateMaskApi(documentId, {
            data: Array.from(fullPng),
          })
        } catch (e) {
          useEditorUiStore.getState().showError(normalizeErrorMessage(e))
        }
      },
      onFinalize: async (_patch, region) => {
        const documentId = useEditorUiStore.getState().currentDocumentId
        if (!documentId) return
        // Compute the inpaint region with margin
        const brushSize = usePreferencesStore.getState().brushConfig.size
        const width = Math.max(brushSize, region.width)
        const height = Math.max(brushSize, region.height)
        const margin = Math.min(width * 0.2, 32)
        const doc = currentDocument!
        const x0 = Math.max(0, Math.floor(region.x - margin))
        const y0 = Math.max(0, Math.floor(region.y - margin))
        const x1 = Math.min(
          doc.width,
          Math.ceil(region.x + region.width + margin),
        )
        const y1 = Math.min(
          doc.height,
          Math.ceil(region.y + region.height + margin),
        )
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
              await inpaintRegionApi(documentId, { region: inpaintRegion })
              await invalidateDocument(documentId)
              useEditorUiStore.getState().setShowInpaintedImage(true)
            } catch (e) {
              useEditorUiStore.getState().showError(normalizeErrorMessage(e))
            }
          })
      },
    },
  )

  // Only allow drawing on the mask if the specific tools are active
  const bind = isActive ? rawBind : () => ({})

  return { canvasRef, visible: showMask, bind }
}
