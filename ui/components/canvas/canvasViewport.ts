'use client'

import { getCachedDocument } from '@/lib/app/documents/queries'
import { getQueryClient } from '@/lib/react-query/client'
import { getEditorUiState } from '@/hooks/ui/useEditorUiState'

const canvasViewportRef: { current: HTMLDivElement | null } = { current: null }

export function setCanvasViewport(element: HTMLDivElement | null) {
  canvasViewportRef.current = element
}

export function fitCanvasToViewport() {
  const { setScale, setAutoFitEnabled, currentDocumentId } = getEditorUiState()
  if (!currentDocumentId) return
  const doc = getCachedDocument(getQueryClient(), currentDocumentId)
  const viewport = canvasViewportRef.current
  if (!doc || !viewport) return
  const rect = viewport.getBoundingClientRect()
  if (!rect.width || !rect.height || !doc.width || !doc.height) return
  const scaleW = ((rect.width - 10) / doc.width) * 100 // leave 10px for margin
  const scaleH = ((rect.height - 10) / doc.height) * 100 // leave 10px for margin
  const fit = Math.max(10, Math.min(100, Math.min(scaleW, scaleH)))
  setAutoFitEnabled(true)
  setScale(fit)
}

export function resetCanvasScale() {
  const { setScale, setAutoFitEnabled } = getEditorUiState()
  setAutoFitEnabled(false)
  setScale(100)
}
