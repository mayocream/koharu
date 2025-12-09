'use client'

import { useAppStore } from '@/lib/store'

export function useCanvasZoom() {
  const {
    scale,
    setScale,
    documents,
    currentDocumentIndex,
    setAutoFitEnabled,
  } = useAppStore()
  const currentDocument = documents[currentDocumentIndex]

  const summary = currentDocument
    ? `${currentDocument.width} x ${currentDocument.height}`
    : '--'

  const applyScale = (value: number) => {
    setAutoFitEnabled(false)
    setScale(value)
  }

  return {
    scale,
    setScale: applyScale,
    summary,
  }
}
