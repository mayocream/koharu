'use client'

import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { useCurrentDocumentState } from '@/lib/query/hooks'

export function useCanvasZoom() {
  const scale = useEditorUiStore((state) => state.scale)
  const setScaleRaw = useEditorUiStore((state) => state.setScale)
  const setAutoFitEnabled = useEditorUiStore((state) => state.setAutoFitEnabled)
  const { currentDocument } = useCurrentDocumentState()

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
