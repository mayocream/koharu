'use client'

import * as React from 'react'

import type { Page } from '@/lib/api/schemas'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'
import type { ToolMode } from '@/lib/types'

type SampleCanvas = {
  data: Uint8Array
  canvas: HTMLCanvasElement
  ctx: CanvasRenderingContext2D
}

type CanvasPointerEvent = React.PointerEvent<HTMLElement>
type CanvasMouseEvent = React.MouseEvent<HTMLElement>
type PointerDocumentEvent = CanvasPointerEvent | CanvasMouseEvent

type DocumentPoint = {
  x: number
  y: number
}

type PointerToDocumentFn = (event: PointerDocumentEvent) => DocumentPoint | null

type UseCanvasEyedropperOptions = {
  mode: ToolMode
  page: Page | null
  imageData?: Uint8Array
  inpaintedData?: Uint8Array
  renderedData?: Uint8Array
  showInpaintedImage: boolean
  showRenderedImage: boolean
  pointerToDocument: PointerToDocumentFn
}

type LatestSample = {
  color: string
  x: number
  y: number
  sampleCanvas: HTMLCanvasElement
}

const MIN_BRUSH_SIZE = 8
const MAX_BRUSH_SIZE = 128
const DEFAULT_BRUSH_SIZE = 32

// Fast anchored scrub. The OS cursor is hidden and the HUD stays at the start point.
const SCRUB_SENSITIVITY = 0.85
const FINE_SCRUB_SENSITIVITY = 0.22

const LEFT_BUTTON = 0
const RIGHT_BUTTON = 2
const PRIMARY_BUTTONS_MASK = 1
const SECONDARY_BUTTONS_MASK = 2

const clampBrushSize = (size: number) =>
  Math.max(MIN_BRUSH_SIZE, Math.min(MAX_BRUSH_SIZE, Math.round(size)))

const componentToHex = (value: number) => value.toString(16).padStart(2, '0').toUpperCase()

const toHex = (r: number, g: number, b: number) =>
  `#${componentToHex(r)}${componentToHex(g)}${componentToHex(b)}`

const isTextInputTarget = (target: EventTarget | null) => {
  if (!(target instanceof HTMLElement)) return false

  return (
    target instanceof HTMLInputElement ||
    target instanceof HTMLTextAreaElement ||
    target instanceof HTMLSelectElement ||
    target.isContentEditable
  )
}

const createImageBitmapFromBytes = async (data: Uint8Array) => {
  const blob = new Blob([data as BlobPart])
  return await createImageBitmap(blob)
}

const drawMagnifier = (
  overlayCanvas: HTMLCanvasElement,
  source: HTMLCanvasElement,
  x: number,
  y: number,
  color: string,
) => {
  const ctx = overlayCanvas.getContext('2d')
  if (!ctx) return

  const sourceSize = 13
  const zoom = 7
  const padding = 8
  const labelHeight = 22
  const size = sourceSize * zoom

  ctx.fillStyle = 'rgba(2, 6, 23, 0.96)'
  ctx.fillRect(0, 0, overlayCanvas.width, overlayCanvas.height)

  const sx = Math.max(0, Math.min(source.width - sourceSize, x - Math.floor(sourceSize / 2)))
  const sy = Math.max(0, Math.min(source.height - sourceSize, y - Math.floor(sourceSize / 2)))

  ctx.imageSmoothingEnabled = false
  ctx.drawImage(source, sx, sy, sourceSize, sourceSize, padding, padding, size, size)

  const center = padding + Math.floor(sourceSize / 2) * zoom + Math.floor(zoom / 2)

  ctx.strokeStyle = 'rgba(255, 255, 255, 0.96)'
  ctx.lineWidth = 1
  ctx.beginPath()
  ctx.moveTo(center, padding)
  ctx.lineTo(center, padding + size)
  ctx.moveTo(padding, center)
  ctx.lineTo(padding + size, center)
  ctx.stroke()

  ctx.strokeStyle = 'rgba(15, 23, 42, 0.95)'
  ctx.strokeRect(padding - 0.5, padding - 0.5, size + 1, size + 1)

  ctx.fillStyle = color
  ctx.fillRect(padding, padding + size + 6, 18, 10)
  ctx.strokeStyle = 'rgba(255, 255, 255, 0.45)'
  ctx.strokeRect(padding + 0.5, padding + size + 6.5, 17, 9)

  ctx.fillStyle = '#f8fafc'
  ctx.font = '11px ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace'
  ctx.textBaseline = 'middle'
  ctx.fillText(color, padding + 24, padding + size + 11)
}

const isAltBrushEvent = (mode: ToolMode, event: CanvasPointerEvent | CanvasMouseEvent) =>
  mode === 'brush' && event.altKey && !isTextInputTarget(event.target)

export function useCanvasEyedropper({
  mode,
  page,
  imageData,
  inpaintedData,
  renderedData,
  showInpaintedImage,
  showRenderedImage,
  pointerToDocument,
}: UseCanvasEyedropperOptions) {
  const setBrushConfig = usePreferencesStore((state) => state.setBrushConfig)
  const brushSize = usePreferencesStore((state) => state.brushConfig.size)

  const sampleCanvasRef = React.useRef<SampleCanvas | null>(null)
  const overlayRef = React.useRef<HTMLCanvasElement | null>(null)
  const cursorRef = React.useRef<HTMLDivElement | null>(null)
  const sizeHudRef = React.useRef<HTMLDivElement | null>(null)
  const latestSampleRef = React.useRef<LatestSample | null>(null)
  const suppressContextMenuRef = React.useRef(false)
  const cursorHiddenRef = React.useRef(false)
  const previousDocumentCursorRef = React.useRef('')
  const previousBodyCursorRef = React.useRef('')

  const altSizeScrubRef = React.useRef<{
    startX: number
    startSize: number
    hudX: number
    hudY: number
  } | null>(null)

  const setCursorHidden = React.useCallback((hidden: boolean) => {
    if (hidden && !cursorHiddenRef.current) {
      previousDocumentCursorRef.current = document.documentElement.style.cursor
      previousBodyCursorRef.current = document.body.style.cursor
      document.documentElement.style.cursor = 'none'
      document.body.style.cursor = 'none'
      cursorHiddenRef.current = true
      return
    }

    if (!hidden && cursorHiddenRef.current) {
      document.documentElement.style.cursor = previousDocumentCursorRef.current
      document.body.style.cursor = previousBodyCursorRef.current
      cursorHiddenRef.current = false
    }
  }, [])

  React.useEffect(() => {
    const overlay = document.createElement('canvas')
    overlay.width = 107
    overlay.height = 129
    overlay.style.position = 'fixed'
    overlay.style.zIndex = '2147483647'
    overlay.style.pointerEvents = 'none'
    overlay.style.display = 'none'
    overlay.style.width = '107px'
    overlay.style.height = '129px'
    overlay.style.borderRadius = '12px'
    overlay.style.overflow = 'hidden'
    overlay.style.border = '1px solid rgba(148, 163, 184, 0.5)'
    overlay.style.boxShadow = '0 14px 40px rgba(0, 0, 0, 0.42)'
    overlay.style.background = 'rgba(2, 6, 23, 0.95)'
    overlay.style.imageRendering = 'pixelated'
    document.body.appendChild(overlay)

    const cursor = document.createElement('div')
    cursor.style.position = 'fixed'
    cursor.style.zIndex = '2147483647'
    cursor.style.pointerEvents = 'none'
    cursor.style.display = 'none'
    cursor.style.width = '18px'
    cursor.style.height = '18px'
    cursor.style.borderRadius = '9999px'
    cursor.style.border = '1px solid rgba(255, 255, 255, 0.96)'
    cursor.style.boxShadow = '0 0 0 1px rgba(15, 23, 42, 0.95), 0 0 12px rgba(0, 0, 0, 0.55)'
    cursor.style.transform = 'translate(-50%, -50%)'
    cursor.innerHTML =
      '<div style="position:absolute;left:50%;top:-5px;width:1px;height:28px;background:rgba(255,255,255,.86);box-shadow:0 0 0 1px rgba(15,23,42,.72);transform:translateX(-50%)"></div><div style="position:absolute;left:-5px;top:50%;width:28px;height:1px;background:rgba(255,255,255,.86);box-shadow:0 0 0 1px rgba(15,23,42,.72);transform:translateY(-50%)"></div><div style="position:absolute;left:50%;top:50%;width:4px;height:4px;border-radius:9999px;background:rgba(255,255,255,.96);box-shadow:0 0 0 1px rgba(15,23,42,.95);transform:translate(-50%,-50%)"></div>'
    document.body.appendChild(cursor)

    const sizeHud = document.createElement('div')
    sizeHud.style.position = 'fixed'
    sizeHud.style.zIndex = '2147483647'
    sizeHud.style.pointerEvents = 'none'
    sizeHud.style.display = 'none'
    sizeHud.style.minWidth = '74px'
    sizeHud.style.padding = '6px 9px'
    sizeHud.style.borderRadius = '12px'
    sizeHud.style.border = '1px solid rgba(148, 163, 184, 0.45)'
    sizeHud.style.boxShadow = '0 12px 30px rgba(0, 0, 0, 0.34)'
    sizeHud.style.background = 'rgba(2, 6, 23, 0.92)'
    sizeHud.style.color = '#f8fafc'
    sizeHud.style.font = '11px ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace'
    sizeHud.style.textAlign = 'center'
    document.body.appendChild(sizeHud)

    overlayRef.current = overlay
    cursorRef.current = cursor
    sizeHudRef.current = sizeHud

    return () => {
      overlay.remove()
      cursor.remove()
      sizeHud.remove()
      overlayRef.current = null
      cursorRef.current = null
      sizeHudRef.current = null
      setCursorHidden(false)
    }
  }, [setCursorHidden])

  const getActiveImageData = React.useCallback(() => {
    if (showRenderedImage && renderedData) return renderedData
    if (showInpaintedImage && inpaintedData) return inpaintedData
    return imageData
  }, [imageData, inpaintedData, renderedData, showInpaintedImage, showRenderedImage])

  const hidePreview = React.useCallback(() => {
    if (overlayRef.current) {
      overlayRef.current.style.display = 'none'
    }

    if (cursorRef.current) {
      cursorRef.current.style.display = 'none'
    }

    setCursorHidden(false)
  }, [setCursorHidden])

  const hideSizeHud = React.useCallback(() => {
    if (sizeHudRef.current) {
      sizeHudRef.current.style.display = 'none'
    }

    setCursorHidden(false)
  }, [setCursorHidden])

  const getSampleCanvas = React.useCallback(async () => {
    if (!page) return null

    const data = getActiveImageData()
    if (!data) return null

    if (sampleCanvasRef.current?.data === data) {
      return sampleCanvasRef.current
    }

    const bitmap = await createImageBitmapFromBytes(data)

    const canvas = document.createElement('canvas')
    canvas.width = page.width || bitmap.width
    canvas.height = page.height || bitmap.height

    const ctx = canvas.getContext('2d', { willReadFrequently: true })
    if (!ctx) return null

    ctx.drawImage(bitmap, 0, 0, canvas.width, canvas.height)
    bitmap.close?.()

    const sampleCanvas = { data, canvas, ctx }
    sampleCanvasRef.current = sampleCanvas
    return sampleCanvas
  }, [getActiveImageData, page])

  const sampleAtEvent = React.useCallback(
    async (event: PointerDocumentEvent) => {
      const point = pointerToDocument(event)
      if (!point) return null

      const sampleCanvas = await getSampleCanvas()
      if (!sampleCanvas) return null

      const x = Math.max(0, Math.min(sampleCanvas.canvas.width - 1, Math.floor(point.x)))
      const y = Math.max(0, Math.min(sampleCanvas.canvas.height - 1, Math.floor(point.y)))
      const pixel = sampleCanvas.ctx.getImageData(x, y, 1, 1).data
      const color = toHex(pixel[0] ?? 0, pixel[1] ?? 0, pixel[2] ?? 0)

      const sample = { color, x, y, sampleCanvas: sampleCanvas.canvas }
      latestSampleRef.current = sample
      return sample
    },
    [getSampleCanvas, pointerToDocument],
  )

  const updatePreview = React.useCallback(
    async (event: CanvasPointerEvent) => {
      const overlay = overlayRef.current
      const cursor = cursorRef.current

      if (!overlay || !cursor || !isAltBrushEvent(mode, event)) {
        hidePreview()
        return
      }

      setCursorHidden(true)

      cursor.style.left = `${event.clientX}px`
      cursor.style.top = `${event.clientY}px`
      cursor.style.display = 'block'

      const sample = await sampleAtEvent(event)
      if (!sample) {
        hidePreview()
        return
      }

      overlay.style.left = `${event.clientX + 18}px`
      overlay.style.top = `${event.clientY + 18}px`
      overlay.style.display = 'block'

      drawMagnifier(overlay, sample.sampleCanvas, sample.x, sample.y, sample.color)
    },
    [hidePreview, mode, sampleAtEvent, setCursorHidden],
  )

  const finishSizeScrub = React.useCallback(() => {
    altSizeScrubRef.current = null
    suppressContextMenuRef.current = false
    hideSizeHud()
  }, [hideSizeHud])

  const showSizeHud = React.useCallback(
    (size: number) => {
      const sizeHud = sizeHudRef.current
      const scrub = altSizeScrubRef.current
      if (!sizeHud || !scrub) return

      setCursorHidden(true)

      sizeHud.innerHTML = `<span style="opacity:.68">Brush</span> <strong>${size}px</strong>`
      sizeHud.style.left = `${scrub.hudX}px`
      sizeHud.style.top = `${scrub.hudY}px`
      sizeHud.style.display = 'block'
    },
    [setCursorHidden],
  )

  const startSizeScrub = React.useCallback(
    (event: CanvasPointerEvent) => {
      const currentSize = brushSize || DEFAULT_BRUSH_SIZE

      altSizeScrubRef.current = {
        startX: event.clientX,
        startSize: currentSize,
        hudX: event.clientX + 18,
        hudY: event.clientY + 18,
      }

      suppressContextMenuRef.current = true
      latestSampleRef.current = null
      hidePreview()
      showSizeHud(currentSize)
    },
    [brushSize, hidePreview, showSizeHud],
  )

  const updateSizeScrub = React.useCallback(
    (event: CanvasPointerEvent) => {
      const scrub = altSizeScrubRef.current
      if (!scrub) return false

      event.preventDefault()
      event.stopPropagation()

      const sensitivity = event.shiftKey ? FINE_SCRUB_SENSITIVITY : SCRUB_SENSITIVITY
      const delta = event.clientX - scrub.startX
      const nextSize = clampBrushSize(scrub.startSize + delta * sensitivity)
      setBrushConfig({ size: nextSize })
      showSizeHud(nextSize)
      return true
    },
    [setBrushConfig, showSizeHud],
  )

  const commitPick = React.useCallback(
    async (event: CanvasPointerEvent) => {
      const sample = latestSampleRef.current ?? (await sampleAtEvent(event))

      if (sample?.color) {
        setBrushConfig({ color: sample.color })
      }

      hidePreview()
    },
    [hidePreview, sampleAtEvent, setBrushConfig],
  )

  const handlePointerDownCapture = React.useCallback(
    (event: CanvasPointerEvent) => {
      if (!isAltBrushEvent(mode, event)) return false

      // Right button always means brush-size scrub. Never let it fall into eyedropper.
      if (event.button === RIGHT_BUTTON || (event.buttons & SECONDARY_BUTTONS_MASK) !== 0) {
        event.preventDefault()
        event.stopPropagation()
        startSizeScrub(event)
        return true
      }

      if (event.button === LEFT_BUTTON) {
        event.preventDefault()
        event.stopPropagation()
        return true
      }

      return false
    },
    [mode, startSizeScrub],
  )

  const handlePointerMoveCapture = React.useCallback(
    (event: CanvasPointerEvent) => {
      // If right button is held, force size scrub even if pointerdown was missed.
      if (mode === 'brush' && event.altKey && (event.buttons & SECONDARY_BUTTONS_MASK) !== 0) {
        event.preventDefault()
        event.stopPropagation()

        if (!altSizeScrubRef.current) {
          startSizeScrub(event)
        }

        updateSizeScrub(event)
        return
      }

      if (updateSizeScrub(event)) return

      if (mode === 'brush' && event.altKey && (event.buttons & PRIMARY_BUTTONS_MASK) === 0) {
        event.preventDefault()
        event.stopPropagation()
        void updatePreview(event)
        return
      }

      hidePreview()
    },
    [hidePreview, mode, startSizeScrub, updatePreview, updateSizeScrub],
  )

  const handlePointerUpCapture = React.useCallback(
    (event: CanvasPointerEvent) => {
      if (altSizeScrubRef.current) {
        event.preventDefault()
        event.stopPropagation()
        finishSizeScrub()
        return true
      }

      if (mode === 'brush' && event.altKey && event.button === LEFT_BUTTON) {
        event.preventDefault()
        event.stopPropagation()

        void commitPick(event)
        return true
      }

      return false
    },
    [commitPick, finishSizeScrub, mode],
  )

  const handlePointerLeave = React.useCallback(() => {
    hidePreview()
    finishSizeScrub()
  }, [finishSizeScrub, hidePreview])

  const handleContextMenuCapture = React.useCallback(
    (event: CanvasMouseEvent) => {
      if (mode !== 'brush') return false

      if (event.altKey || suppressContextMenuRef.current) {
        event.preventDefault()
        event.stopPropagation()
        finishSizeScrub()
        return true
      }

      return false
    },
    [finishSizeScrub, mode],
  )

  React.useEffect(() => {
    if (mode !== 'brush') {
      latestSampleRef.current = null
      hidePreview()
      finishSizeScrub()
    }
  }, [finishSizeScrub, hidePreview, mode])

  return {
    handlePointerDownCapture,
    handlePointerMoveCapture,
    handlePointerUpCapture,
    handlePointerLeave,
    handleContextMenuCapture,
  }
}
