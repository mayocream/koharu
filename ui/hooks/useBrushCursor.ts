'use client'

import { useEffect, useRef, useMemo } from 'react'
import type React from 'react'

import { usePreferencesStore } from '@/lib/stores/preferencesStore'

export function useBrushCursor(
  canvasRef: React.RefObject<HTMLDivElement | null>,
  mode: string,
  pageKey?: string,
) {
  const brushCursorRef = useRef<HTMLDivElement>(null)
  const cachedRectRef = useRef<DOMRect | null>(null)
  const mousePosRef = useRef<{ x: number; y: number } | null>(null)
  const isInsideRef = useRef(false)
  const brushSize = usePreferencesStore((state) => state.brushConfig.size)

  const isBrushMode = useMemo(
    () => mode === 'brush' || mode === 'repairBrush' || mode === 'eraser',
    [mode],
  )

  const isBrushModeRef = useRef(isBrushMode)
  useEffect(() => {
    isBrushModeRef.current = isBrushMode
  }, [isBrushMode])

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return

    const refresh = () => {
      cachedRectRef.current = canvas.getBoundingClientRect()
      if (mousePosRef.current) {
        updateCursorPosition(mousePosRef.current.x, mousePosRef.current.y)
      }
    }

    const updateCursorPosition = (clientX: number, clientY: number) => {
      if (!brushCursorRef.current) return
      const rect = cachedRectRef.current || canvas.getBoundingClientRect()
      if (!cachedRectRef.current) cachedRectRef.current = rect
      const x = clientX - rect.left
      const y = clientY - rect.top
      brushCursorRef.current.style.transform = `translate(${x}px, ${y}px) translate(-50%, -50%)`
    }

    const handleMove = (e: PointerEvent) => {
      mousePosRef.current = { x: e.clientX, y: e.clientY }
      updateCursorPosition(e.clientX, e.clientY)
    }

    const handleEnter = () => {
      isInsideRef.current = true
      refresh()
      if (isBrushModeRef.current && brushCursorRef.current) {
        brushCursorRef.current.style.opacity = '1'
      }
    }

    const handleLeave = () => {
      isInsideRef.current = false
      if (brushCursorRef.current) {
        brushCursorRef.current.style.opacity = '0'
      }
    }

    const resizeObserver = new ResizeObserver(() => refresh())
    resizeObserver.observe(canvas)

    canvas.addEventListener('pointermove', handleMove)
    canvas.addEventListener('pointerenter', handleEnter)
    canvas.addEventListener('pointerleave', handleLeave)
    window.addEventListener('scroll', refresh, true)
    window.addEventListener('resize', refresh)

    // Initial positioning if we already have a mouse position
    if (mousePosRef.current) {
      updateCursorPosition(mousePosRef.current.x, mousePosRef.current.y)
    }

    return () => {
      resizeObserver.disconnect()
      canvas.removeEventListener('pointermove', handleMove)
      canvas.removeEventListener('pointerenter', handleEnter)
      canvas.removeEventListener('pointerleave', handleLeave)
      window.removeEventListener('scroll', refresh, true)
      window.removeEventListener('resize', refresh)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [canvasRef, pageKey])

  // Separate effect for visibility to avoid re-attaching listeners
  useEffect(() => {
    const cursor = brushCursorRef.current
    if (!cursor) return

    if (isBrushMode && isInsideRef.current) {
      cursor.style.opacity = '1'
    } else {
      cursor.style.opacity = '0'
    }
  }, [isBrushMode])

  return { brushCursorRef, isBrushMode, brushSize }
}
