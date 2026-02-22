'use client'

import { useAppShallow } from '@/lib/store-selectors'

export function useCanvasZoom() {
  const { scale, setScale, currentDocument, setAutoFitEnabled } = useAppShallow(
    (state) => ({
      scale: state.scale,
      setScale: state.setScale,
      currentDocument: state.currentDocument,
      setAutoFitEnabled: state.setAutoFitEnabled,
    }),
  )

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
