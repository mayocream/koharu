'use client'

import { useGetDocument } from '@/lib/api/documents/documents'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'

export function useCanvasZoom() {
  const scale = useEditorUiStore((state) => state.scale)
  const setScaleRaw = useEditorUiStore((state) => state.setScale)
  const setAutoFitEnabled = useEditorUiStore((state) => state.setAutoFitEnabled)
  const documentId = useEditorUiStore((s) => s.currentDocumentId)
  const { data: document } = useGetDocument(documentId ?? '', {
    query: { enabled: !!documentId },
  })

  const summary = document ? `${document.width} x ${document.height}` : '--'

  const applyScale = (value: number) => {
    setAutoFitEnabled(false)
    setScaleRaw(value)
  }

  return {
    scale,
    setScale: applyScale,
    summary,
  }
}
