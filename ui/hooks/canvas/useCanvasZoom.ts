'use client'

import { useDocumentView } from '@/hooks/documents/useDocumentView'
import { useEditorUiState } from '@/hooks/ui/useEditorUiState'

export function useCanvasZoom() {
  const scale = useEditorUiState((state) => state.scale)
  const setScaleRaw = useEditorUiState((state) => state.setScale)
  const setAutoFitEnabled = useEditorUiState((state) => state.setAutoFitEnabled)
  const { currentDocument } = useDocumentView()

  const summary = currentDocument
    ? `${currentDocument.width} x ${currentDocument.height}`
    : '--'

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
