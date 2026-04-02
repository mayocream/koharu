'use client'

import type { RefObject } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'
import { updateBrushLayer } from '@/lib/api/regions/regions'
import {
  getGetDocumentQueryKey,
  getListDocumentsQueryKey,
} from '@/lib/api/documents/documents'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import type { ToolMode } from '@/types'
import type { MappedDocument } from '@/hooks/useTextBlocks'
import type { PointerToDocumentFn } from '@/hooks/usePointerToDocument'
import { useCanvasDrawing } from '@/hooks/useCanvasDrawing'

type RenderBrushOptions = {
  mode: ToolMode
  currentDocument: MappedDocument | null
  pointerToDocument: PointerToDocumentFn
  enabled: boolean
  action: 'paint' | 'erase'
  targetCanvasRef?: RefObject<HTMLCanvasElement | null>
}

export function useRenderBrushDrawing({
  currentDocument,
  pointerToDocument,
  enabled,
  action,
  targetCanvasRef,
}: RenderBrushOptions) {
  const queryClient = useQueryClient()
  const isErasing = action === 'erase'

  return useCanvasDrawing(currentDocument, pointerToDocument, {
    getColor: () =>
      isErasing ? '#000000' : usePreferencesStore.getState().brushConfig.color,
    blendMode: isErasing ? 'destination-out' : 'source-over',
    getBrushSize: () => usePreferencesStore.getState().brushConfig.size,
    enabled,
    targetCanvasRef,
    clearAfterStroke: true,
    onFinalize: async (patch, region) => {
      const documentId = useEditorUiStore.getState().currentDocumentId
      if (!documentId) return
      await updateBrushLayer(documentId, {
        data: Array.from(patch),
        region,
      })
      await queryClient.invalidateQueries({
        queryKey: getGetDocumentQueryKey(documentId),
      })
      await queryClient.invalidateQueries({
        queryKey: getListDocumentsQueryKey(),
      })
      useEditorUiStore.getState().setShowBrushLayer(true)
    },
  })
}
