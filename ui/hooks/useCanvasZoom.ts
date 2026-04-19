'use client'

import { useCurrentPage } from '@/hooks/useCurrentPage'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'

export function useCanvasZoom() {
  const scale = useEditorUiStore((s) => s.scale)
  const setScaleRaw = useEditorUiStore((s) => s.setScale)
  const setAutoFitEnabled = useEditorUiStore((s) => s.setAutoFitEnabled)
  const page = useCurrentPage()
  const summary = page ? `${page.width} x ${page.height}` : '--'

  const applyScale = (value: number) => {
    setAutoFitEnabled(false)
    setScaleRaw(value)
  }

  return { scale, setScale: applyScale, summary }
}
