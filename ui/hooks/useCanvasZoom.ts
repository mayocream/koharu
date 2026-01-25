'use client'

import { useAppStore } from '@/lib/store'

export function useCanvasZoom() {
  const { scale, setScale, currentDocument, setAutoFitEnabled } = useAppStore()

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
