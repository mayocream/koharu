'use client'

import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { CanvasOverlay } from '@/components/canvas/CanvasOverlay'
import { CanvasToolbar } from '@/components/canvas/CanvasToolbar'
import { SubToolRail } from '@/components/canvas/SubToolRail'
import { ToolRail } from '@/components/canvas/ToolRail'
import {
  isTextElement,
  koharuClient,
  useEditorStore,
  type CanvasDisplay,
  type CanvasMaskOverlay,
  type EntityId,
  type Frame,
  type TransformFrame,
} from '@/lib/koharu'
import {
  draftFrame,
  entityFrame,
  hitTestEntities,
  pagePoint,
  scrollCamera,
  translateFrames,
  zoomAtPoint,
} from '@/lib/koharu/geometry'

interface ElementDrag {
  pointer: number
  start: [number, number]
  originals: TransformFrame[]
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
  const drag = useRef<ElementDrag | null>(null)
  const textDraft = useRef<TextDraft | null>(null)
  const pan = useRef<PanDrag | null>(null)
  const masking = useRef<number | null>(null)
  const spaceHeld = useRef(false)
  const nativeTransform = useRef(false)
  const transformFrame = useRef(0)
  const transformSession = useRef(0)
  const [previews, setPreviews] = useState<Record<EntityId, Frame>>({})
  const [draft, setDraft] = useState<Frame | null>(null)
  const [cursor, setCursor] = useState<[number, number] | null>(null)

  const page = useEditorStore((state) => state.page)
  const tool = useEditorStore((state) => state.tool)
  const selectedElements = useEditorStore((state) => state.selectedElements)
  const hoveredElement = useEditorStore((state) => state.hoveredElement)
  const display = useEditorStore((state) => state.display)
  const brushSize = useEditorStore((state) => state.brushSize)
  const camera = useEditorStore((state) => state.camera)

  const cancelGesture = useCallback(() => {
    if (masking.current !== null) koharuClient.interact({ type: 'cancel_mask_stroke' })
    if (nativeTransform.current) koharuClient.interact({ type: 'cancel_transform' })
    nativeTransform.current = false
    transformSession.current += 1
    drag.current = null
    textDraft.current = null
    pan.current = null
    masking.current = null
    setPreviews({})
    setDraft(null)
  }, [])

  const reportViewport = useCallback(() => {
    if (surface.current) koharuClient.reportViewport(surface.current)
  }, [])

  const beginTransform = useCallback((elements: TransformFrame[]) => {
    if (!elements.length || nativeTransform.current) return
    nativeTransform.current = true
    transformSession.current += 1
    transformFrame.current = 0
    setPreviews(Object.fromEntries(elements.map(({ element, frame }) => [element, frame])))
    koharuClient.interact({ type: 'begin_transform', elements: elements.map(({ element }) => element) })
  }, [])

  const updateTransform = useCallback((elements: TransformFrame[]) => {
    if (!nativeTransform.current) return
    transformFrame.current += 1
    setPreviews(Object.fromEntries(elements.map(({ element, frame }) => [element, frame])))
    koharuClient.interact({
      type: 'update_transform',
      frame: transformFrame.current,
      elements,
    })
  }, [])

  const finishTransform = useCallback(() => {
    if (!nativeTransform.current) return
    nativeTransform.current = false
    const session = transformSession.current
    void koharuClient
      .command({ type: 'finish_transform' })
      .catch(() => undefined)
      .finally(() => {
        if (!nativeTransform.current && transformSession.current === session) setPreviews({})
      })
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
        state.selectElements(state.page.entities.map((entity) => entity.id))
      } else if (
        (event.key === 'Delete' || event.key === 'Backspace') &&
        state.page &&
        state.selectedElements.length
      ) {
        event.preventDefault()
        koharuClient.fire({ type: 'delete_entities', entities: state.selectedElements })
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

  function selectable(entity: NonNullable<typeof page>['entities'][number]): boolean {
    return entity.image !== null || isTextElement(entity)
  }

  function framesFor(elements: EntityId[]): TransformFrame[] {
    const state = useEditorStore.getState()
    return elements.flatMap((element) => {
      const entity = state.page?.entities.find((candidate) => candidate.id === element)
      const frame =
        entity && selectable(entity) && entity.visibility.visible && entity.visibility.opacity > 0
          ? previews[element] ?? entityFrame(entity)
          : null
      return frame ? [{ element, frame }] : []
    })
  }

  function handlePointerDown(event: React.PointerEvent<HTMLDivElement>) {
    if (!page || event.button > 1) return
    if (event.target instanceof Element && event.target.closest('.moveable-control')) return
    event.currentTarget.setPointerCapture(event.pointerId)
    const state = useEditorStore.getState()
    const point = physicalPoint(event.clientX, event.clientY)
    if (state.tool === 'text_mask' || state.tool === 'brush_mask') setCursor(point)

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
      const start = pageCoordinates(event.clientX, event.clientY)
      const target = hitTestEntities(page.entities, start, selectable)
      if (!target) {
        if (!event.shiftKey && !event.ctrlKey && !event.metaKey) state.selectElements([])
        return
      }
      const additive = event.shiftKey || event.ctrlKey || event.metaKey
      const selected = additive
        ? state.selectedElements.includes(target.id)
          ? state.selectedElements.filter((id) => id !== target.id)
          : [...state.selectedElements, target.id]
        : state.selectedElements.includes(target.id)
          ? state.selectedElements
          : [target.id]
      state.selectElements(selected)
      if (!selected.includes(target.id)) return
      const originals = framesFor(selected)
      drag.current = { pointer: event.pointerId, start, originals }
      beginTransform(originals)
    } else if (state.tool === 'text') {
      const start = pageCoordinates(event.clientX, event.clientY)
      const frame = draftFrame(start, start)
      textDraft.current = { pointer: event.pointerId, start, frame }
      setDraft(frame)
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
    const state = useEditorStore.getState()
    if (state.tool === 'text_mask' || state.tool === 'brush_mask') setCursor(point)

    if (pan.current?.pointer === event.pointerId) {
      const translation: [number, number] = [
        pan.current.translation[0] + point[0] - pan.current.start[0],
        pan.current.translation[1] + point[1] - pan.current.start[1],
      ]
      useEditorStore.setState({ camera: { zoom: state.camera.zoom, translation, autoFit: false } })
      koharuClient.interact({ type: 'set_camera', zoom: state.camera.zoom, translation })
      return
    }
    if (masking.current === event.pointerId) {
      koharuClient.interact({ type: 'extend_mask_stroke', x: point[0], y: point[1] })
      return
    }

    const currentDraft = textDraft.current
    if (currentDraft?.pointer === event.pointerId) {
      currentDraft.frame = draftFrame(
        currentDraft.start,
        pageCoordinates(event.clientX, event.clientY),
      )
      setDraft(currentDraft.frame)
      return
    }

    const currentDrag = drag.current
    if (currentDrag?.pointer === event.pointerId) {
      const current = pageCoordinates(event.clientX, event.clientY)
      updateTransform(
        translateFrames(currentDrag.originals, [
          current[0] - currentDrag.start[0],
          current[1] - currentDrag.start[1],
        ]),
      )
      return
    }

    if (state.tool === 'select') {
      const target = hitTestEntities(
        page.entities,
        pageCoordinates(event.clientX, event.clientY),
        selectable,
      )
      const hovered = target?.id ?? null
      if (hovered !== state.hoveredElement) state.setHoveredElement(hovered)
    }
  }

  function handlePointerUp(event: React.PointerEvent<HTMLDivElement>) {
    if (!page) return
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
      setDraft(null)
      koharuClient.fire({ type: 'add_text', page: page.id, frame })
    }

    if (drag.current?.pointer === event.pointerId) {
      const current = pageCoordinates(event.clientX, event.clientY)
      updateTransform(
        translateFrames(drag.current.originals, [
          current[0] - drag.current.start[0],
          current[1] - drag.current.start[1],
        ]),
      )
      drag.current = null
      finishTransform()
    }
  }

  function handleWheel(event: React.WheelEvent<HTMLDivElement>) {
    if (!page) return
    event.preventDefault()
    const state = useEditorStore.getState()
    const next = event.ctrlKey
      ? zoomAtPoint(
          state.camera,
          physicalPoint(event.clientX, event.clientY),
          Math.min(16, Math.max(0.02, state.camera.zoom * Math.exp(-event.deltaY * 0.0015))),
        )
      : scrollCamera(state.camera, [event.deltaX, event.deltaY])
    useEditorStore.setState({ camera: { ...next, autoFit: false } })
    koharuClient.interact({ type: 'set_camera', ...next })
  }

  const textCursor = tool === 'text' ? 'crosshair' : tool === 'pan' ? 'grab' : undefined
  const hasSelectedText = page?.entities.some(
    (entity) => selectedElements.includes(entity.id) && isTextElement(entity),
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
            setCursor(null)
            useEditorStore.getState().setHoveredElement(null)
          }}
          onWheel={handleWheel}
        >
          {page && (
            <CanvasOverlay
              page={page}
              camera={camera}
              selected={selectedElements}
              hovered={hoveredElement}
              previews={previews}
              draft={draft}
              cursor={cursor}
              brushSize={brushSize}
              showBrushCursor={tool === 'text_mask' || tool === 'brush_mask'}
              onTransformStart={beginTransform}
              onTransformFrame={updateTransform}
              onTransformEnd={finishTransform}
            />
          )}
        </div>
      </div>
    </main>
  )
}
