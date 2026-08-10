'use client'

import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useColorSampling } from '@/components/controls/ColorSampling'
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
import { receiveError, useKoharuStore, type CanvasTool } from '@/lib/store'

const canvasCursors = {
  select: undefined,
  text: 'text',
  draw: 'none',
  eraser: 'none',
  color_picker: 'crosshair',
  remove: 'none',
  pan: 'grab',
} as const satisfies Record<CanvasTool, string | undefined>

type Gesture =
  | { kind: 'pan'; pointer: number; start: Point; translation: [number, number] }
  | { kind: 'move'; pointer: number; start: Point; originals: TransformFrame[] }
  | { kind: 'text'; pointer: number; start: Point; frame: Frame }
  | { kind: 'paint'; pointer: number }
  | { kind: 'erase'; pointer: number }
  | { kind: 'inpaint'; pointer: number }

interface CanvasView {
  zoom: number
  translation: [number, number]
}

interface StrokeUpdate {
  kind: 'paint' | 'erase' | 'inpaint'
  points: Point[]
}

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
  const colorSampling = useColorSampling()

  const page = usePage().data
  const camera = useKoharuStore((state) => state.camera)
  const layerFrames = useKoharuStore((state) => state.layerFrames)
  const tool = useKoharuStore((state) => state.tool)
  const brush = useKoharuStore((state) => state.brush)
  const selected = useKoharuStore((state) => state.selectedLayers)
  const selectLayers = useKoharuStore((state) => state.selectLayers)
  const setTool = useKoharuStore((state) => state.setTool)
  const setBrush = useKoharuStore((state) => state.setBrush)
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

  const viewportUpdates = useFrameCommand((element: HTMLElement) => updateViewport(element))
  const viewUpdates = useFrameCommand(({ zoom, translation }: CanvasView) =>
    call(commands.setCanvasView, zoom, translation).then(() => undefined),
  )
  const transformUpdates = useFrameCommand((elements: TransformFrame[]) => {
    transformFrame.current += 1
    return enqueue(() => call(commands.updateTransform, transformFrame.current, elements)).then(
      () => undefined,
    )
  })
  const strokeUpdates = useFrameCommand(({ kind, points }: StrokeUpdate) => {
    if (kind === 'paint') {
      return enqueue(() => call(commands.extendPaint, points)).then(() => undefined)
    }
    if (kind === 'erase') {
      return enqueue(() => call(commands.extendErase, points)).then(() => undefined)
    }
    return enqueue(() => call(commands.extendInpaint, points)).then(() => undefined)
  }, mergeStrokeUpdates)

  const report = useCallback(() => {
    if (surface.current) viewportUpdates.schedule(surface.current)
  }, [viewportUpdates])

  const beginTransform = useCallback(
    (elements: TransformFrame[]) => {
      if (!elements.length || transformActive.current) return
      transformUpdates.clear()
      transformActive.current = true
      transformFrame.current = 0
      setPreviews(Object.fromEntries(elements.map(({ element, frame }) => [element, frame])))
      void enqueue(() => call(commands.beginTransform, elements)).catch(() => undefined)
    },
    [enqueue, transformUpdates],
  )

  const updateTransform = useCallback(
    (elements: TransformFrame[]) => {
      if (!transformActive.current) return
      setPreviews(Object.fromEntries(elements.map(({ element, frame }) => [element, frame])))
      transformUpdates.schedule(elements)
    },
    [transformUpdates],
  )

  const finishTransform = useCallback(() => {
    if (!transformActive.current) return
    transformUpdates.commit()
    transformActive.current = false
    void enqueue(() => call(commands.finishTransform))
      .then((revision) => (revision === null ? undefined : refresh(projectKey, pagesKey, pageKey)))
      .catch(() => undefined)
      .finally(() => setPreviews({}))
  }, [enqueue, transformUpdates])

  const cancelGesture = useCallback(() => {
    const current = gesture.current
    gesture.current = null
    if (current?.kind === 'paint' || current?.kind === 'erase' || current?.kind === 'inpaint') {
      strokeUpdates.clear()
    }
    if (current?.kind === 'paint') {
      void enqueue(() => call(commands.cancelPaint)).catch(() => undefined)
    } else if (current?.kind === 'erase') {
      void enqueue(() => call(commands.cancelErase)).catch(() => undefined)
    } else if (current?.kind === 'inpaint') {
      void enqueue(() => call(commands.cancelInpaint)).catch(() => undefined)
    }
    if (transformActive.current) {
      transformUpdates.clear()
      transformActive.current = false
      void enqueue(() => call(commands.cancelTransform)).catch(() => undefined)
    }
    setDraft(null)
    setPreviews({})
  }, [enqueue, strokeUpdates, transformUpdates])

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
        colorSampling?.cancel()
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
  }, [cancelGesture, colorSampling, page, selectLayers, setTool])

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
      viewUpdates.schedule({ zoom: camera.zoom, translation })
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
      strokeUpdates.schedule({ kind: 'paint', points })
    } else if (current.kind === 'erase') {
      strokeUpdates.schedule({ kind: 'erase', points })
    } else if (current.kind === 'inpaint') {
      strokeUpdates.schedule({ kind: 'inpaint', points })
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
      strokeUpdates.commit()
      void enqueue(() => call(commands.finishPaint))
        .then((result) => {
          selectLayers([result.layer])
          return refresh(projectKey, pagesKey, pageKey)
        })
        .catch(() => undefined)
    } else if (current.kind === 'erase') {
      strokeUpdates.commit()
      void enqueue(() => call(commands.finishErase))
        .then((result) => {
          selectLayers([result.layer])
          return refresh(projectKey, pagesKey, pageKey)
        })
        .catch(() => undefined)
    } else if (current.kind === 'inpaint') {
      strokeUpdates.commit()
      void enqueue(() => call(commands.finishInpaint)).catch(() => undefined)
    }
  }

  return (
    <main className='relative flex h-full min-h-0 min-w-0 flex-col overflow-hidden rounded-tl-2xl bg-transparent'>
      <CanvasCommandBar />
      <div className='relative flex min-h-0 min-w-0 flex-1'>
        <ToolBar />
        <div
          ref={surface}
          tabIndex={0}
          aria-label={t('canvas.surface')}
          className='relative min-h-0 min-w-0 flex-1 touch-none overflow-hidden bg-transparent outline-none'
          style={{ cursor: page ? canvasCursors[tool] : undefined }}
          onContextMenu={(event) => event.preventDefault()}
          onPointerDown={(event) => {
            if (!page || event.button > 1) return
            if (event.target instanceof Element && event.target.closest('[data-canvas-control]'))
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
              strokeUpdates.clear()
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
              strokeUpdates.clear()
              gesture.current = { kind: 'erase', pointer: event.pointerId }
              void enqueue(() =>
                call(commands.beginErase, activeRaster.id, point, brush.diameter),
              ).catch(() => undefined)
            } else if (tool === 'remove') {
              strokeUpdates.clear()
              gesture.current = { kind: 'inpaint', pointer: event.pointerId }
              void enqueue(() => call(commands.beginInpaint, point, brush.diameter)).catch(
                () => undefined,
              )
            } else if (tool === 'color_picker') {
              void call(commands.sampleColor, physical)
                .then((color) => {
                  const hex = rgbaToHex(color)
                  if (!colorSampling?.complete(hex)) setBrush({ ...brush, color: hex })
                })
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
            viewUpdates.schedule({ zoom, translation })
          }}
        >
          {page && (
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
              <p className='text-[12px] text-muted-foreground'>{t('canvas.empty')}</p>
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

function mergeStrokeUpdates(current: StrokeUpdate, next: StrokeUpdate): StrokeUpdate {
  if (current.kind !== next.kind) return next
  current.points.push(...next.points)
  return current
}

function useFrameCommand<Value>(
  execute: (value: Value) => Promise<unknown>,
  merge?: (current: Value, next: Value) => Value,
): FrameCommand<Value> {
  const executeRef = useRef(execute)
  executeRef.current = execute
  const command = useRef<FrameCommand<Value> | null>(null)
  command.current ??= new FrameCommand((value) => executeRef.current(value), merge)

  useEffect(() => () => command.current?.clear(), [])
  return command.current
}

class FrameCommand<Value> {
  private pending: Value | undefined
  private frame: number | null = null

  constructor(
    private readonly execute: (value: Value) => Promise<unknown>,
    private readonly merge: (current: Value, next: Value) => Value = (_current, next) => next,
  ) {}

  schedule(value: Value): void {
    this.pending = this.pending === undefined ? value : this.merge(this.pending, value)
    if (this.frame !== null) return
    this.frame = requestAnimationFrame(() => {
      this.frame = null
      this.executePending()
    })
  }

  commit(): void {
    if (this.frame !== null) {
      cancelAnimationFrame(this.frame)
      this.frame = null
    }
    this.executePending()
  }

  clear(): void {
    if (this.frame !== null) cancelAnimationFrame(this.frame)
    this.frame = null
    this.pending = undefined
  }

  private executePending(): void {
    const value = this.pending
    if (value === undefined) return
    this.pending = undefined
    void this.execute(value).catch(() => undefined)
  }
}
