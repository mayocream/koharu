'use client'

import { useAppStore } from '@/lib/store'

const canvasViewportRef: { current: HTMLDivElement | null } = { current: null }

export function setCanvasViewport(element: HTMLDivElement | null) {
  canvasViewportRef.current = element
}

export function fitCanvasToViewport() {
  const { documents, currentDocumentIndex, setScale, setAutoFitEnabled } =
    useAppStore.getState()
  const doc = documents[currentDocumentIndex]
  const viewport = canvasViewportRef.current
  if (!doc || !viewport) return
  const rect = viewport.getBoundingClientRect()
  if (!rect.width || !rect.height || !doc.width || !doc.height) return
  const scaleW = (rect.width / doc.width) * 100
  const scaleH = (rect.height / doc.height) * 100
  const fit = Math.max(
    10,
    Math.min(100, Math.floor(Math.min(scaleW, scaleH) / 10) * 10),
  )
  setAutoFitEnabled(true)
  setScale(fit)
}

export function resetCanvasScale() {
  const { setScale } = useAppStore.getState()
  setScale(100)
}
