'use client'

import { useEditorUiStore } from '@/lib/stores/editorUiStore'

const canvasViewportRef: { current: HTMLDivElement | null } = { current: null }
let docSize: { width: number; height: number } | null = null

export function setCanvasViewport(element: HTMLDivElement | null) {
  canvasViewportRef.current = element
}

export function setCanvasDocumentSize(width: number, height: number) {
  docSize = { width, height }
}

export function fitCanvasToViewport() {
  const viewport = canvasViewportRef.current
  if (!docSize || !viewport) return
  const rect = viewport.getBoundingClientRect()
  if (!rect.width || !rect.height || !docSize.width || !docSize.height) return
  const scaleW = ((rect.width - 10) / docSize.width) * 100
  const scaleH = ((rect.height - 10) / docSize.height) * 100
  const fit = Math.max(10, Math.min(100, Math.min(scaleW, scaleH)))
  useEditorUiStore.getState().setAutoFitEnabled(true)
  useEditorUiStore.getState().setScale(fit)
}

export function resetCanvasScale() {
  useEditorUiStore.getState().setAutoFitEnabled(false)
  useEditorUiStore.getState().setScale(100)
}
