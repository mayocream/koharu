'use client'

import { useCallback, useEffect, useRef, useState } from 'react'

import {
  activateCanvas,
  cancelCanvasPrefetch,
  createCanvas,
  fetchCanvasManifest,
  fetchCanvasResource,
  type Canvas,
} from '@koharu/bridge/canvas'

export type CanvasStatus = 'loading' | 'switching' | 'ready' | 'recovering' | 'error'

interface CanvasState {
  canvas: Canvas | null
  error: Error | null
  generation: number | null
  hasFrame: boolean
  retry: () => void
  status: CanvasStatus
}

export function useCanvas(
  element: HTMLCanvasElement | null,
  revision: number | null,
  generation: number,
): CanvasState {
  const [canvasAttempt, setCanvasAttempt] = useState(0)
  const [loadAttempt, setLoadAttempt] = useState(0)
  const [canvas, setCanvas] = useState<Canvas | null>(null)
  const [status, setStatus] = useState<CanvasStatus>('loading')
  const [error, setError] = useState<Error | null>(null)
  const [activeGeneration, setActiveGeneration] = useState<number | null>(null)
  const [hasFrame, setHasFrame] = useState(false)
  const hasFrameRef = useRef(false)
  const generationRef = useRef(generation)
  const revisionRef = useRef(revision)
  const activeFrame = useRef<{
    canvas: Canvas
    generation: number
  } | null>(null)
  const request = useRef<object | null>(null)
  generationRef.current = generation
  revisionRef.current = revision

  const updateHasFrame = useCallback((value: boolean) => {
    hasFrameRef.current = value
    setHasFrame(value)
  }, [])

  const retry = useCallback(() => {
    if (canvas) setLoadAttempt((value) => value + 1)
    else setCanvasAttempt((value) => value + 1)
  }, [canvas])

  useEffect(() => {
    if (!element) return
    let active = true
    let owned: Canvas | null = null
    setCanvas(null)
    setError(null)
    setActiveGeneration(null)
    updateHasFrame(false)
    setStatus(canvasAttempt === 0 ? 'loading' : 'recovering')

    void createCanvas(element, () => {
      if (!active) return
      setStatus('recovering')
      setCanvasAttempt((value) => value + 1)
    })
      .then((created) => {
        if (!active) {
          created.dispose()
          return
        }
        owned = created
        setCanvas(created)
      })
      .catch((reason: unknown) => {
        if (!active) return
        setError(toError(reason))
        setStatus('error')
      })

    return () => {
      active = false
      request.current = null
      activeFrame.current = null
      owned?.dispose()
    }
  }, [canvasAttempt, element, updateHasFrame])

  useEffect(() => {
    if (!canvas) return
    if (revision === null) {
      cancelCanvasPrefetch()
      request.current = null
      if (
        !hasFrameRef.current &&
        activeFrame.current?.canvas === canvas &&
        activeFrame.current.generation === generation
      )
        return
      try {
        canvas.clear()
        activeFrame.current = { canvas, generation }
        setActiveGeneration(generation)
        updateHasFrame(false)
        setError(null)
        setStatus('ready')
      } catch (reason) {
        setError(toError(reason))
        setStatus('error')
      }
      return
    }
    if (
      generation === 0 ||
      (activeFrame.current?.canvas === canvas && activeFrame.current.generation === generation)
    )
      return

    const requested = generation
    cancelCanvasPrefetch()
    const currentRequest = {}
    request.current = currentRequest
    setError(null)
    setStatus((current) =>
      hasFrameRef.current ? 'switching' : current === 'recovering' ? 'recovering' : 'loading',
    )

    const current = () =>
      request.current === currentRequest &&
      generationRef.current === requested &&
      revisionRef.current !== null

    void prepareFrame(canvas, requested, current)
      .then((activated) => {
        if (!current() || !activated) return
        activeFrame.current = { canvas, generation: requested }
        setActiveGeneration(requested)
        request.current = null
        updateHasFrame(true)
        setError(null)
        setStatus('ready')
      })
      .catch((reason: unknown) => {
        if (!current()) return
        request.current = null
        setError(toError(reason))
        setStatus('error')
      })
  }, [canvas, generation, loadAttempt, revision, updateHasFrame])

  useEffect(() => {
    if (!canvas) return
    return activateCanvas(canvas)
  }, [canvas])

  return { canvas, error, generation: activeGeneration, hasFrame, retry, status }
}

async function prepareFrame(
  canvas: Canvas,
  generation: number,
  current: () => boolean,
): Promise<boolean> {
  const manifest = await fetchCanvasManifest(generation)
  if (!current()) return false
  if (canvas.hasActiveManifest(manifest)) return true
  const staged = canvas.stageManifest(manifest)
  for (let offset = 0; offset < staged.missing.length; offset += 4) {
    const resources = staged.missing.slice(offset, offset + 4)
    const packets = await Promise.all(
      resources.map(
        async (resource) => [resource, await fetchCanvasResource(generation, resource)] as const,
      ),
    )
    if (!current()) return false
    await Promise.all(packets.map(([resource, packet]) => canvas.installResource(resource, packet)))
  }
  if (!current()) return false
  const activated = canvas.activateFrame(staged.token)
  if (!current()) return false
  if (!activated) throw new Error('The prepared canvas frame was superseded before activation.')
  return true
}

function toError(value: unknown): Error {
  if (value instanceof Error) return value
  return new Error(typeof value === 'string' ? value : 'Could not initialize the WebGPU canvas.')
}
