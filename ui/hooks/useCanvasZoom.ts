'use client'

import { useMemo } from 'react'
import { useAppStore } from '@/lib/store'

export function useCanvasZoom() {
  const { scale, setScale, documents, currentDocumentIndex, setAutoFitEnabled } =
    useAppStore()
  const currentDocument = documents[currentDocumentIndex]

  const summary = useMemo(() => {
    if (!currentDocument) return '--'
    return `${currentDocument.width} x ${currentDocument.height}`
  }, [currentDocument])

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
