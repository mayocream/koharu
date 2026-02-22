'use client'

import { queryKeys } from '@/lib/query/keys'
import { getQueryClient } from '@/lib/query/client'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import type { Document } from '@/types'

const canvasViewportRef: { current: HTMLDivElement | null } = { current: null }

export function setCanvasViewport(element: HTMLDivElement | null) {
  canvasViewportRef.current = element
}

export function fitCanvasToViewport() {
  const { setScale, setAutoFitEnabled, currentDocumentIndex } =
    useEditorUiStore.getState()
  const doc = getQueryClient().getQueryData<Document>(
    queryKeys.documents.current(currentDocumentIndex),
  )
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
  const { setScale, setAutoFitEnabled } = useEditorUiStore.getState()
  setAutoFitEnabled(false)
  setScale(100)
}
