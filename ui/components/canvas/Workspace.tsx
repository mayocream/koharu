'use client'

import { useCallback, useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'

import { CanvasToolbar } from '@/components/canvas/CanvasToolbar'
import { SubToolRail } from '@/components/canvas/SubToolRail'
import { ToolRail } from '@/components/canvas/ToolRail'
import {
  isTextElement,
  koharuClient,
  useEditorStore,
  type CanvasDisplay,
  type CanvasMaskOverlay,
  type Frame,
  type HitTarget,
} from '@/lib/koharu'
import { draftFrame, pagePoint, transformFrame, zoomAtPoint } from '@/lib/koharu/geometry'

interface PendingHit {
  id: number
  pointer: number
  start: [number, number]
  additive: boolean
  released: boolean
}

interface ElementDrag {
  pointer: number
  start: [number, number]
  target: HitTarget
  frames: Map<string, Frame>
}

interface TextDraft {
  pointer: number
  start: [number, number]
  frame: Frame
}

interface PanDrag {
  pointer: number
  start: [number, number]
  translation: [number, number]
}

export function Workspace() {
  const { t } = useTranslation()
  const surface = useRef<HTMLDivElement>(null)
  const hitSequence = useRef(0)
  const pendingHit = useRef<PendingHit | null>(null)
  const hoverHit = useRef<number | null>(null)
  const drag = useRef<ElementDrag | null>(null)
  const textDraft = useRef<TextDraft | null>(null)
  const pan = useRef<PanDrag | null>(null)
  const masking = useRef<number | null>(null)
  const previewFrames = useRef<Map<string, Frame>>(new Map())
  const cursor = useRef<[number, number] | null>(null)
  const spaceHeld = useRef(false)

  const page = useEditorStore((state) => state.page)
  const tool = useEditorStore((state) => state.tool)
  const selectedElements = useEditorStore((state) => state.selectedElements)
  const hoveredElement = useEditorStore((state) => state.hoveredElement)
  const showTextBounds = useEditorStore((state) => state.showTextBounds)
  const display = useEditorStore((state) => state.display)
  const brushSize = useEditorStore((state) => state.brushSize)

  const sendOverlays = useCallback(() => {
    const state = useEditorStore.getState()
    const draft = textDraft.current?.frame ?? null
    const showCursor = state.tool === 'text_mask' || state.tool === 'brush_mask'
    koharuClient.interact({
      type: 'set_overlays',
      selected: state.selectedElements,
      hovered: state.hoveredElement,
      previews: [...previewFrames.current].map(([element, frame]) => ({ element, frame })),
      draft,
      guides: [],
      show_text_bounds: state.showTextBounds,
      brush_cursor:
        showCursor && cursor.current
          ? { x: cursor.current[0], y: cursor.current[1], diameter: state.brushSize }
          : null,
    })
  }, [])

  const cancelGesture = useCallback(() => {
    if (masking.current !== null) koharuClient.interact({ type: 'cancel_mask_stroke' })
    pendingHit.current = null
    drag.current = null
    textDraft.current = null
    pan.current = null
    masking.current = null
    previewFrames.current.clear()
    sendOverlays()
  }, [sendOverlays])

  const reportViewport = useCallback(() => {
    if (surface.current) koharuClient.reportViewport(surface.current)
  }, [])

  useEffect(() => {
    const element = surface.current
    if (!element) return
    reportViewport()
    const observer = new ResizeObserver(reportViewport)
    const themeObserver = new MutationObserver(reportViewport)
    observer.observe(element)
    themeObserver.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ['class'],
    })
    window.addEventListener('resize', reportViewport)
    window.visualViewport?.addEventListener('resize', reportViewport)
    return () => {
      observer.disconnect()
      themeObserver.disconnect()
      window.removeEventListener('resize', reportViewport)
      window.visualViewport?.removeEventListener('resize', reportViewport)
    }
  }, [reportViewport])

  useEffect(() => {
    if (!page) return
    let next = display
    if (display.page === 'clean' && !page.assets.clean) next = { ...display, page: 'source' }
    if (display.page === 'rendered' && !page.assets.rendered) next = { ...display, page: 'source' }
    if (next !== display) useEditorStore.getState().setDisplay(next)
    koharuClient.interact({ type: 'set_display', display: next })
  }, [display, page])

  useEffect(sendOverlays, [
    brushSize,
    hoveredElement,
    selectedElements,
    sendOverlays,
    showTextBounds,
    tool,
  ])

  useEffect(() => {
    return koharuClient.subscribe((event) => {
      if (
        event.type === 'project_opened' ||
        event.type === 'project_closed' ||
        event.type === 'page_loaded' ||
        event.type === 'project_changed' ||
        (event.type === 'rejected' && event.error.code === 'stale_revision')
      ) {
        cancelGesture()
      }
      if (event.type !== 'hit_test') return
      if (event.id === hoverHit.current) {
        hoverHit.current = null
        useEditorStore.getState().setHoveredElement(event.target?.element ?? null)
        return
      }
      const pending = pendingHit.current
      if (!pending || pending.id !== event.id) return
      pendingHit.current = null
      const state = useEditorStore.getState()
      const target = event.target
      if (!target) {
        if (!pending.additive) state.selectElements([])
        return
      }

      const selected = pending.additive
        ? state.selectedElements.includes(target.element)
          ? state.selectedElements.filter((id) => id !== target.element)
          : [...state.selectedElements, target.element]
        : state.selectedElements.includes(target.element)
          ? state.selectedElements
          : [target.element]
      state.selectElements(selected)

      if (pending.released || !state.page) return
      const frames = new Map<string, Frame>()
      for (const element of state.page.elements) {
        if (selected.includes(element.id)) frames.set(element.id, element.frame)
      }
      drag.current = { pointer: pending.pointer, start: pending.start, target, frames }
    })
  }, [cancelGesture])

  useEffect(() => {
    const editable = (target: EventTarget | null) =>
      target instanceof HTMLInputElement ||
      target instanceof HTMLTextAreaElement ||
      (target instanceof HTMLElement && target.isContentEditable)
    const keydown = (event: KeyboardEvent) => {
      if (editable(event.target)) return
      const state = useEditorStore.getState()
      if (event.code === 'Space') {
        spaceHeld.current = true
        event.preventDefault()
        return
      }
      const command = event.ctrlKey || event.metaKey
      if (command && event.key.toLowerCase() === 'z') {
        event.preventDefault()
        koharuClient.fire({ type: event.shiftKey ? 'redo' : 'undo' })
      } else if (command && event.key.toLowerCase() === 'a' && state.page) {
        event.preventDefault()
        state.selectElements(state.page.elements.map((element) => element.id))
      } else if (
        (event.key === 'Delete' || event.key === 'Backspace') &&
        state.page &&
        state.selectedElements.length
      ) {
        event.preventDefault()
        koharuClient.fire({
          type: 'delete_elements',
          page: state.page.id,
          elements: state.selectedElements,
        })
      } else if (event.key.toLowerCase() === state.shortcuts.fit) {
        koharuClient.interact({ type: 'fit_window' })
      } else if (event.key === 'Escape') {
        cancelGesture()
        state.selectElements([])
      } else {
        const next = (['select', 'text', 'text_mask', 'brush_mask', 'pan'] as const).find(
          (action) => state.shortcuts[action] === event.key.toLowerCase(),
        )
        if (next) state.setTool(next)
      }
    }
    const keyup = (event: KeyboardEvent) => {
      if (event.code === 'Space') spaceHeld.current = false
    }
    const blur = () => {
      spaceHeld.current = false
      cancelGesture()
    }
    window.addEventListener('keydown', keydown)
    window.addEventListener('keyup', keyup)
    window.addEventListener('blur', blur)
    return () => {
      window.removeEventListener('keydown', keydown)
      window.removeEventListener('keyup', keyup)
      window.removeEventListener('blur', blur)
    }
  }, [cancelGesture])

  function physicalPoint(clientX: number, clientY: number): [number, number] {
    const bounds = surface.current!.getBoundingClientRect()
    return [
      (clientX - bounds.x) * window.devicePixelRatio,
      (clientY - bounds.y) * window.devicePixelRatio,
    ]
  }

  function pageCoordinates(clientX: number, clientY: number): [number, number] {
    return pagePoint(
      clientX,
      clientY,
      surface.current!.getBoundingClientRect(),
      useEditorStore.getState().camera,
    )
  }

  function handlePointerDown(event: React.PointerEvent<HTMLDivElement>) {
    if (!page || event.button > 1) return
    event.currentTarget.setPointerCapture(event.pointerId)
    const state = useEditorStore.getState()
    const point = physicalPoint(event.clientX, event.clientY)
    cursor.current = point

    if (event.button === 1 || state.tool === 'pan' || spaceHeld.current) {
      pan.current = {
        pointer: event.pointerId,
        start: point,
        translation: state.camera.translation,
      }
      event.preventDefault()
      return
    }

    if (state.tool === 'select') {
      const id = ++hitSequence.current
      pendingHit.current = {
        id,
        pointer: event.pointerId,
        start: pageCoordinates(event.clientX, event.clientY),
        additive: event.shiftKey || event.ctrlKey || event.metaKey,
        released: false,
      }
      koharuClient.interact({ type: 'hit_test', id, x: point[0], y: point[1] })
    } else if (state.tool === 'text') {
      const start = pageCoordinates(event.clientX, event.clientY)
      textDraft.current = { pointer: event.pointerId, start, frame: draftFrame(start, start) }
      sendOverlays()
    } else {
      masking.current = event.pointerId
      const plane = state.tool === 'text_mask' ? 'text' : 'brush'
      const overlay: CanvasMaskOverlay = {
        tint: plane === 'text' ? [244, 63, 94, 210] : [14, 165, 233, 210],
        opacity: 0.55,
      }
      const nextDisplay: CanvasDisplay =
        plane === 'text'
          ? { ...state.display, text_mask: overlay }
          : { ...state.display, brush_mask: overlay }
      state.setDisplay(nextDisplay)
      koharuClient.interact({ type: 'set_display', display: nextDisplay })
      koharuClient.interact({
        type: 'begin_mask_stroke',
        plane,
        diameter: state.brushSize,
        erase: state.erase,
        x: point[0],
        y: point[1],
      })
    }
  }

  function handlePointerMove(event: React.PointerEvent<HTMLDivElement>) {
    if (!page) return
    const point = physicalPoint(event.clientX, event.clientY)
    cursor.current = point
    const state = useEditorStore.getState()

    if (pan.current?.pointer === event.pointerId) {
      const dx = point[0] - pan.current.start[0]
      const dy = point[1] - pan.current.start[1]
      const translation: [number, number] = [
        pan.current.translation[0] + dx,
        pan.current.translation[1] + dy,
      ]
      koharuClient.interact({ type: 'set_camera', zoom: state.camera.zoom, translation })
      return
    }
    if (masking.current === event.pointerId) {
      koharuClient.interact({ type: 'extend_mask_stroke', x: point[0], y: point[1] })
      sendOverlays()
      return
    }

    const currentDraft = textDraft.current
    if (currentDraft?.pointer === event.pointerId) {
      currentDraft.frame = draftFrame(
        currentDraft.start,
        pageCoordinates(event.clientX, event.clientY),
      )
      sendOverlays()
      return
    }

    const currentDrag = drag.current
    if (currentDrag?.pointer === event.pointerId) {
      const now = pageCoordinates(event.clientX, event.clientY)
      const dx = now[0] - currentDrag.start[0]
      const dy = now[1] - currentDrag.start[1]
      previewFrames.current.clear()
      for (const [id, frame] of currentDrag.frames) {
        const target =
          id === currentDrag.target.element
            ? currentDrag.target
            : ({ type: 'element', element: id } as const)
        previewFrames.current.set(id, transformFrame(frame, target, dx, dy))
      }
      sendOverlays()
      return
    }

    sendOverlays()
    if (state.tool === 'select' && pendingHit.current === null && hoverHit.current === null) {
      const id = ++hitSequence.current
      hoverHit.current = id
      koharuClient.interact({ type: 'hit_test', id, x: point[0], y: point[1] })
    }
  }

  function handlePointerUp(event: React.PointerEvent<HTMLDivElement>) {
    if (!page) return
    if (pendingHit.current?.pointer === event.pointerId) pendingHit.current.released = true
    if (pan.current?.pointer === event.pointerId) pan.current = null
    if (masking.current === event.pointerId) {
      masking.current = null
      koharuClient.interact({ type: 'finish_mask_stroke' })
    }

    if (textDraft.current?.pointer === event.pointerId) {
      let frame = textDraft.current.frame
      const click =
        frame.width < 4 / useEditorStore.getState().camera.zoom &&
        frame.height < 4 / useEditorStore.getState().camera.zoom
      if (click) {
        frame = {
          x: textDraft.current.start[0],
          y: textDraft.current.start[1],
          width: Math.min(320, page.size.width * 0.4),
          height: Math.min(120, page.size.height * 0.15),
          angle_degrees: 0,
        }
      }
      textDraft.current = null
      koharuClient.fire({ type: 'add_text', page: page.id, frame })
    }

    if (drag.current?.pointer === event.pointerId) {
      const frames = [...previewFrames.current].map(([element, frame]) => ({
        page: page.id,
        element,
        frame,
      }))
      drag.current = null
      previewFrames.current.clear()
      if (frames.length) koharuClient.fire({ type: 'set_element_frames', elements: frames })
    }
    sendOverlays()
  }

  function handleWheel(event: React.WheelEvent<HTMLDivElement>) {
    if (!page) return
    event.preventDefault()
    const state = useEditorStore.getState()
    if (event.ctrlKey || event.metaKey || Math.abs(event.deltaY) >= Math.abs(event.deltaX)) {
      const point = physicalPoint(event.clientX, event.clientY)
      const nextZoom = Math.min(
        16,
        Math.max(0.02, state.camera.zoom * Math.exp(-event.deltaY * 0.0015)),
      )
      const camera = zoomAtPoint(state.camera, point, nextZoom)
      koharuClient.interact({ type: 'set_camera', ...camera })
    } else {
      koharuClient.interact({
        type: 'set_camera',
        zoom: state.camera.zoom,
        translation: [
          state.camera.translation[0] - event.deltaX * window.devicePixelRatio,
          state.camera.translation[1] - event.deltaY * window.devicePixelRatio,
        ],
      })
    }
  }

  const textCursor = tool === 'text' ? 'crosshair' : tool === 'pan' ? 'grab' : undefined
  const hasSelectedText = page?.elements.some(
    (element) => selectedElements.includes(element.id) && isTextElement(element),
  )

  return (
    <main className='relative flex min-h-0 min-w-0 flex-1 overflow-hidden bg-transparent'>
      <ToolRail />
      <SubToolRail />
      <div className='relative flex min-h-0 min-w-0 flex-1 flex-col'>
        <CanvasToolbar />
        <div
          ref={surface}
          className='relative min-h-0 min-w-0 flex-1 touch-none bg-transparent outline-none'
          style={{ cursor: textCursor }}
          tabIndex={0}
          aria-label={t('native.canvas.surface', { defaultValue: 'Koharu canvas' })}
          data-has-selected-text={hasSelectedText || undefined}
          onContextMenu={(event) => event.preventDefault()}
          onPointerDown={handlePointerDown}
          onPointerMove={handlePointerMove}
          onPointerUp={handlePointerUp}
          onPointerCancel={cancelGesture}
          onPointerLeave={() => {
            cursor.current = null
            useEditorStore.getState().setHoveredElement(null)
            sendOverlays()
          }}
          onWheel={handleWheel}
        />
      </div>
    </main>
  )
}
