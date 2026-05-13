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
  const fit = Math.max(10, Math.min(400, Math.min(scaleW, scaleH)))
  useEditorUiStore.getState().setAutoFitEnabled(true)
  useEditorUiStore.getState().setScale(fit)
  requestAnimationFrame(() => {
    const vp = canvasViewportRef.current
    if (!vp) return
    vp.scrollLeft = (vp.scrollWidth - vp.clientWidth) / 2
    vp.scrollTop = (vp.scrollHeight - vp.clientHeight) / 2
  })
}

export function resetCanvasScale() {
  useEditorUiStore.getState().setAutoFitEnabled(false)
  useEditorUiStore.getState().setScale(100)
}

export function zoomAroundViewportCenter(nextScale: number) {
  const viewport = canvasViewportRef.current
  const store = useEditorUiStore.getState()

  if (!viewport || !docSize) {
    store.setAutoFitEnabled(false)
    store.setScale(nextScale)
    return
  }

  const oldScale = store.scale
  const clamped = Math.max(10, Math.min(400, Math.round(nextScale)))

  // Capture all values before zooming
  const scrollLeft     = viewport.scrollLeft
  const scrollTop      = viewport.scrollTop
  const clientWidth    = viewport.clientWidth
  const clientHeight   = viewport.clientHeight
  const oldScrollWidth = viewport.scrollWidth
  const oldScrollHeight = viewport.scrollHeight

  // Scroll space coordinates for the viewport center
  const centerX = scrollLeft + clientWidth / 2
  const centerY = scrollTop  + clientHeight / 2

  // Scroll space coordinates of the canvas top-left before zooming (canvas is centered)
  const oldCanvasW = docSize.width  * (oldScale / 100)
  const oldCanvasH = docSize.height * (oldScale / 100)
  const oldCanvasLeft = (oldScrollWidth  - oldCanvasW) / 2
  const oldCanvasTop  = (oldScrollHeight - oldCanvasH) / 2

  // Canvas pixel coordinates pointed to by the viewport center
  const pixelX = (centerX - oldCanvasLeft) / (oldScale / 100)
  const pixelY = (centerY - oldCanvasTop)  / (oldScale / 100)

  store.setAutoFitEnabled(false)
  store.setScale(clamped)

  requestAnimationFrame(() => {
    const vp = canvasViewportRef.current
    if (!vp || !docSize) return

    // Canvas position after zooming
    const newCanvasW = docSize.width  * (clamped / 100)
    const newCanvasH = docSize.height * (clamped / 100)
    const newCanvasLeft = (vp.scrollWidth  - newCanvasW) / 2
    const newCanvasTop  = (vp.scrollHeight - newCanvasH) / 2

    // Correct scroll position so the same canvas pixel stays at the center
    vp.scrollLeft = pixelX * (clamped / 100) + newCanvasLeft - clientWidth  / 2
    vp.scrollTop  = pixelY * (clamped / 100) + newCanvasTop  - clientHeight / 2
  })
}
