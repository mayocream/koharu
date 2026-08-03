'use client'

import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { CanvasCommandBar } from '@/components/editor/CanvasCommandBar'
import { CanvasOverlay } from '@/components/editor/CanvasOverlay'
import { StatusBar } from '@/components/editor/StatusBar'
import { ToolBar } from '@/components/editor/ToolBar'
import { call, dispatch, updateViewport } from '@/lib/backend'
import { expandLayerSelection } from '@/lib/document'
import {
  controlFrame,
  draftFrame,
  hitTestLayers,
  pagePoint,
  physicalPoint,
  selectableLayer,
  translateFrames,
} from '@/lib/geometry'
import { commands, type Frame, type Point, type TransformFrame } from '@/lib/protocol'
import { pageKey, pagesKey, projectKey, refresh, usePage } from '@/lib/queries'
import { receiveError, useKoharuStore } from '@/lib/store'

type Gesture =
  | { kind: 'pan'; pointer: number; start: Point; translation: [number, number] }
  | { kind: 'move'; pointer: number; start: Point; originals: TransformFrame[] }
  | { kind: 'text'; pointer: number; start: Point; frame: Frame }
  | { kind: 'paint'; pointer: number }
  | { kind: 'erase'; pointer: number }
  | { kind: 'inpaint'; pointer: number }

export function CanvasWorkspace() {
  const { t } = useTranslation()
  const surface = useRef<HTMLDivElement>(null)
  const gesture = useRef<Gesture | null>(null)
  const spaceHeld = useRef(false)
  const transformActive = useRef(false)
  const transformFrame = useRef(0)
  const commandQueue = useRef<Promise<void>>(Promise.resolve())
  const [previews, setPreviews] = useState<Record<string, Frame>>({})
  const [draft, setDraft] = useState<Frame | null>(null)
  const [hovered, setHovered] = useState<string | null>(null)
  const [cursor, setCursor] = useState<Point | null>(null)

  const page = usePage().data
  const camera = useKoharuStore((state) => state.camera)
  const layerFrames = useKoharuStore((state) => state.layerFrames)
  const tool = useKoharuStore((state) => state.tool)
  const brush = useKoharuStore((state) => state.brush)
  const selected = useKoharuStore((state) => state.selectedLayers)
  const presentation = useKoharuStore((state) => state.presentation)
  const selectLayers = useKoharuStore((state) => state.selectLayers)
  const setTool = useKoharuStore((state) => state.setTool)
  const setBrush = useKoharuStore((state) => state.setBrush)
  const setPresentation = useKoharuStore((state) => state.setPresentation)
  const activeRaster =
    selected.length === 1
      ? page?.layers.find((layer) => layer.id === selected[0] && layer.type === 'raster')
      : undefined

  const enqueue = useCallback(<Result,>(operation: () => Promise<Result>): Promise<Result> => {
    const pending = commandQueue.current.then(operation)
    commandQueue.current = pending.then(
      () => undefined,
      () => undefined,
    )
    return pending
  }, [])

  const report = useCallback(() => {
    if (surface.current) updateViewport(surface.current)
  }, [])

  const beginTransform = useCallback(
    (elements: TransformFrame[]) => {
      if (!elements.length || transformActive.current) return
      transformActive.current = true
      transformFrame.current = 0
      setPreviews(Object.fromEntries(elements.map(({ element, frame }) => [element, frame])))
      void enqueue(() => call(commands.beginTransform, elements)).catch(() => undefined)
    },
    [enqueue],
  )

  const updateTransform = useCallback(
    (elements: TransformFrame[]) => {
      if (!transformActive.current) return
      transformFrame.current += 1
      setPreviews(Object.fromEntries(elements.map(({ element, frame }) => [element, frame])))
      void enqueue(() => call(commands.updateTransform, transformFrame.current, elements)).catch(
        () => undefined,
      )
    },
    [enqueue],
  )

  const finishTransform = useCallback(() => {
    if (!transformActive.current) return
    transformActive.current = false
    void enqueue(() => call(commands.finishTransform))
      .then((revision) => (revision === null ? undefined : refresh(projectKey, pagesKey, pageKey)))
      .catch(() => undefined)
      .finally(() => setPreviews({}))
  }, [enqueue])

  const cancelGesture = useCallback(() => {
    const current = gesture.current
    gesture.current = null
    if (current?.kind === 'paint') {
      void enqueue(() => call(commands.cancelPaint)).catch(() => undefined)
    } else if (current?.kind === 'erase') {
      void enqueue(() => call(commands.cancelErase)).catch(() => undefined)
    } else if (current?.kind === 'inpaint') {
      void enqueue(() => call(commands.cancelInpaint)).catch(() => undefined)
    }
    if (transformActive.current) {
      transformActive.current = false
      void enqueue(() => call(commands.cancelTransform)).catch(() => undefined)
    }
    setDraft(null)
    setPreviews({})
  }, [enqueue])

  useEffect(() => {
    const element = surface.current
    if (!element) return
    report()
    const resize = new ResizeObserver(report)
    const theme = new MutationObserver(report)
    resize.observe(element)
    theme.observe(document.documentElement, { attributes: true, attributeFilter: ['class'] })
    window.addEventListener('resize', report)
    window.visualViewport?.addEventListener('resize', report)
    return () => {
      resize.disconnect()
      theme.disconnect()
      window.removeEventListener('resize', report)
      window.visualViewport?.removeEventListener('resize', report)
    }
  }, [report])

  useEffect(() => {
    if (!page) return
    let next = presentation
    if (next.image === 'rendered' && !page.assets.rendered) next = { ...next, image: 'source' }
    if (next !== presentation) setPresentation(next)
    dispatch(commands.setPresentation, next)
  }, [page, presentation, setPresentation])

  useEffect(() => cancelGesture, [cancelGesture, page?.id, tool])

  useEffect(() => {
    const editable = (target: EventTarget | null) =>
      target instanceof HTMLInputElement ||
      target instanceof HTMLTextAreaElement ||
      (target instanceof HTMLElement && target.isContentEditable)

    const down = (event: KeyboardEvent) => {
      if (editable(event.target)) return
      const state = useKoharuStore.getState()
      if (event.code === 'Space') {
        spaceHeld.current = true
        event.preventDefault()
        return
      }
      const command = event.ctrlKey || event.metaKey
      if (command && event.key.toLowerCase() === 'z') {
        event.preventDefault()
        void call(event.shiftKey ? commands.redo : commands.undo)
          .then(() => refresh(projectKey, pagesKey, pageKey))
          .catch(() => undefined)
        return
      }
      if (command && event.key.toLowerCase() === 'a' && page) {
        event.preventDefault()
        selectLayers(page.layers.filter(selectableLayer).map((layer) => layer.id))
        return
      }
      if (
        (event.key === 'Delete' || event.key === 'Backspace') &&
        state.selectedLayers.length > 0
      ) {
        event.preventDefault()
        void call(commands.deleteLayers, state.selectedLayers)
          .then(() => refresh(projectKey, pagesKey, pageKey))
          .catch(() => undefined)
        return
      }
      if (event.key.toLowerCase() === state.shortcuts.fit) {
        dispatch(commands.fitCanvas)
        return
      }
      if (event.key === 'Escape') {
        cancelGesture()
        selectLayers([])
        return
      }
      const next = (
        ['select', 'text', 'draw', 'eraser', 'color_picker', 'remove', 'pan'] as const
      ).find((action) => state.shortcuts[action] === event.key.toLowerCase())
      if (next) setTool(next)
    }

    const up = (event: KeyboardEvent) => {
      if (event.code === 'Space') spaceHeld.current = false
    }
    const blur = () => {
      spaceHeld.current = false
      cancelGesture()
    }

    window.addEventListener('keydown', down)
    window.addEventListener('keyup', up)
    window.addEventListener('blur', blur)
    return () => {
      window.removeEventListener('keydown', down)
      window.removeEventListener('keyup', up)
      window.removeEventListener('blur', blur)
    }
  }, [cancelGesture, page, selectLayers, setTool])

  const clientPagePoint = (clientX: number, clientY: number) =>
    pagePoint(
      clientX,
      clientY,
      surface.current!.getBoundingClientRect(),
      useKoharuStore.getState().camera,
    )

  const clientPhysicalPoint = (clientX: number, clientY: number) =>
    physicalPoint(clientX, clientY, surface.current!.getBoundingClientRect())

  const framesFor = (layers: string[]): TransformFrame[] =>
    expandLayerSelection(page?.layers ?? [], layers).flatMap((id) => {
      const layer = page?.layers.find((candidate) => candidate.id === id)
      const frame = layer && selectableLayer(layer) ? controlFrame(layer, layerFrames) : null
      return frame ? [{ element: id, frame }] : []
    })

  const moveGesture = (
    pointer: number,
    samples: ReadonlyArray<{ clientX: number; clientY: number }>,
  ) => {
    const current = gesture.current
    const sample = samples.at(-1)
    if (!page || !sample) return
    const physical = clientPhysicalPoint(sample.clientX, sample.clientY)
    setCursor(physical)
    if (!current || current.pointer !== pointer) {
      if (tool === 'select') {
        setHovered(
          hitTestLayers(page.layers, clientPagePoint(sample.clientX, sample.clientY), layerFrames)
            ?.id ?? null,
        )
      }
      return
    }

    if (current.kind === 'pan') {
      const translation: [number, number] = [
        current.translation[0] + physical.x - current.start.x,
        current.translation[1] + physical.y - current.start.y,
      ]
      useKoharuStore.setState({ camera: { zoom: camera.zoom, translation, fitted: false } })
      dispatch(commands.setCanvasView, camera.zoom, translation)
      return
    }

    const points = samples.map((value) => clientPagePoint(value.clientX, value.clientY))
    const point = points.at(-1)!
    if (current.kind === 'move') {
      updateTransform(
        translateFrames(current.originals, {
          x: point.x - current.start.x,
          y: point.y - current.start.y,
        }),
      )
    } else if (current.kind === 'text') {
      current.frame = draftFrame(current.start, point)
      setDraft(current.frame)
    } else if (current.kind === 'paint') {
      void enqueue(() => call(commands.extendPaint, points)).catch(() => undefined)
    } else if (current.kind === 'erase') {
      void enqueue(() => call(commands.extendErase, points)).catch(() => undefined)
    } else if (current.kind === 'inpaint') {
      void enqueue(() => call(commands.extendInpaint, points)).catch(() => undefined)
    }
  }

  const finishGesture = () => {
    const current = gesture.current
    gesture.current = null
    if (!current || !page) return
    if (current.kind === 'move') {
      finishTransform()
    } else if (current.kind === 'text') {
      const pointText =
        current.frame.width < 4 / camera.zoom && current.frame.height < 4 / camera.zoom
      setDraft(null)
      void (
        pointText
          ? call(commands.addPointText, current.start)
          : call(commands.addTextBox, current.frame)
      )
        .then((result) => {
          selectLayers([result.layer])
          return refresh(projectKey, pagesKey, pageKey)
        })
        .catch(() => undefined)
    } else if (current.kind === 'paint') {
      void enqueue(() => call(commands.finishPaint))
        .then((result) => {
          selectLayers([result.layer])
          return refresh(projectKey, pagesKey, pageKey)
        })
        .catch(() => undefined)
    } else if (current.kind === 'erase') {
      void enqueue(() => call(commands.finishErase))
        .then((result) => {
          selectLayers([result.layer])
          return refresh(projectKey, pagesKey, pageKey)
        })
        .catch(() => undefined)
    } else if (current.kind === 'inpaint') {
      void enqueue(() => call(commands.finishInpaint)).catch(() => undefined)
    }
  }

  const cursorStyle =
    tool === 'text' || tool === 'color_picker' ? 'crosshair' : tool === 'pan' ? 'grab' : undefined

  return (
    <main className='relative flex h-full min-h-0 min-w-0 flex-col overflow-hidden rounded-tl-2xl bg-transparent'>
      <CanvasCommandBar />
      <div className='relative flex min-h-0 min-w-0 flex-1'>
        <ToolBar />
        <div
          ref={surface}
          tabIndex={0}
          aria-label={t('native.canvas.surface', { defaultValue: 'Koharu canvas' })}
          className='relative min-h-0 min-w-0 flex-1 touch-none overflow-hidden bg-transparent outline-none'
          style={{ cursor: cursorStyle }}
          onContextMenu={(event) => event.preventDefault()}
          onPointerDown={(event) => {
            if (!page || event.button > 1) return
            if (
              event.target instanceof Element &&
              event.target.closest('.moveable-control, .moveable-line.moveable-direction')
            )
              return
            event.currentTarget.focus()
            event.currentTarget.setPointerCapture(event.pointerId)
            const physical = clientPhysicalPoint(event.clientX, event.clientY)
            const point = clientPagePoint(event.clientX, event.clientY)
            setCursor(physical)

            if (event.button === 1 || tool === 'pan' || spaceHeld.current) {
              gesture.current = {
                kind: 'pan',
                pointer: event.pointerId,
                start: physical,
                translation: camera.translation,
              }
            } else if (presentation.image === 'rendered') {
              return
            } else if (tool === 'select') {
              const target = hitTestLayers(page.layers, point, layerFrames)
              const additive = event.shiftKey || event.ctrlKey || event.metaKey
              if (!target) {
                if (!additive) selectLayers([])
                return
              }
              const next = additive
                ? selected.includes(target.id)
                  ? selected.filter((id) => id !== target.id)
                  : [...selected, target.id]
                : selected.includes(target.id)
                  ? selected
                  : [target.id]
              selectLayers(next)
              if (!next.includes(target.id)) return
              const originals = framesFor(next)
              if (!originals.length) return
              gesture.current = { kind: 'move', pointer: event.pointerId, start: point, originals }
              beginTransform(originals)
            } else if (tool === 'text') {
              const frame = draftFrame(point, point)
              gesture.current = { kind: 'text', pointer: event.pointerId, start: point, frame }
              setDraft(frame)
            } else if (tool === 'draw') {
              gesture.current = { kind: 'paint', pointer: event.pointerId }
              void enqueue(() =>
                call(commands.beginPaint, activeRaster?.id ?? null, point, {
                  diameter: brush.diameter,
                  color: hexToRgba(brush.color),
                }),
              ).catch(() => undefined)
            } else if (tool === 'eraser') {
              if (!activeRaster) {
                receiveError('Select a paint or cleanup layer before using the Eraser.')
                return
              }
              gesture.current = { kind: 'erase', pointer: event.pointerId }
              void enqueue(() =>
                call(commands.beginErase, activeRaster.id, point, brush.diameter),
              ).catch(() => undefined)
            } else if (tool === 'remove') {
              gesture.current = { kind: 'inpaint', pointer: event.pointerId }
              void enqueue(() => call(commands.beginInpaint, point, brush.diameter)).catch(
                () => undefined,
              )
            } else if (tool === 'color_picker') {
              void call(commands.sampleColor, physical)
                .then((color) => setBrush({ ...brush, color: rgbaToHex(color) }))
                .catch(() => undefined)
            }
            event.preventDefault()
          }}
          onPointerMove={(event) => {
            if (!page) return
            const coalesced = event.nativeEvent.getCoalescedEvents?.() ?? [event.nativeEvent]
            moveGesture(event.pointerId, coalesced.length ? coalesced : [event.nativeEvent])
          }}
          onPointerUp={(event) => {
            moveGesture(event.pointerId, [event.nativeEvent])
            finishGesture()
            if (event.currentTarget.hasPointerCapture(event.pointerId)) {
              event.currentTarget.releasePointerCapture(event.pointerId)
            }
          }}
          onPointerCancel={() => cancelGesture()}
          onPointerLeave={(event) => {
            if (!event.currentTarget.hasPointerCapture(event.pointerId)) {
              setHovered(null)
              setCursor(null)
            }
          }}
          onWheel={(event) => {
            if (!page) return
            event.preventDefault()
            const point = clientPhysicalPoint(event.clientX, event.clientY)
            const current = useKoharuStore.getState().camera
            let zoom = current.zoom
            let translation = current.translation
            if (event.ctrlKey) {
              zoom = clamp(current.zoom * Math.exp(-event.deltaY * 0.0015), 0.02, 16)
              const pageX = (point.x - current.translation[0]) / current.zoom
              const pageY = (point.y - current.translation[1]) / current.zoom
              translation = [point.x - pageX * zoom, point.y - pageY * zoom]
            } else {
              const dpr = window.devicePixelRatio
              translation = [
                current.translation[0] - event.deltaX * dpr,
                current.translation[1] - event.deltaY * dpr,
              ]
            }
            useKoharuStore.setState({ camera: { zoom, translation, fitted: false } })
            dispatch(commands.setCanvasView, zoom, translation)
          }}
        >
          {page && presentation.image !== 'rendered' && (
            <CanvasOverlay
              page={page}
              camera={camera}
              selected={selected}
              hovered={hovered}
              frames={layerFrames}
              previews={previews}
              draft={draft}
              cursor={cursor}
              brushSize={brush.diameter}
              showBrushCursor={tool === 'draw' || tool === 'eraser' || tool === 'remove'}
              onTransformStart={beginTransform}
              onTransformFrame={updateTransform}
              onTransformEnd={finishTransform}
            />
          )}
          {!page && (
            <div className='pointer-events-none absolute inset-0 grid place-items-center'>
              <p className='text-[12px] text-muted-foreground'>Select or import a page</p>
            </div>
          )}
        </div>
      </div>
      <StatusBar />
    </main>
  )
}

function hexToRgba(hex: string): [number, number, number, number] {
  const value = hex.replace('#', '')
  return [
    Number.parseInt(value.slice(0, 2), 16),
    Number.parseInt(value.slice(2, 4), 16),
    Number.parseInt(value.slice(4, 6), 16),
    255,
  ]
}

function rgbaToHex(color: [number, number, number, number]): string {
  return `#${color
    .slice(0, 3)
    .map((channel) => channel.toString(16).padStart(2, '0'))
    .join('')}`.toUpperCase()
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.min(maximum, Math.max(minimum, value))
}
