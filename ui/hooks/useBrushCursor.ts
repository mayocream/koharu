'use client'

import { useCallback, useEffect, useRef, useMemo } from 'react'
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
  const altPressedRef = useRef(false)
  const updateCursorPositionRef = useRef<((clientX: number, clientY: number) => void) | null>(null)
  const brushSize = usePreferencesStore((state) => state.brushConfig.size)

  const isBrushMode = useMemo(
    () => mode === 'brush' || mode === 'repairBrush' || mode === 'eraser',
    [mode],
  )

  const syncVisibility = useCallback(() => {
    const cursor = brushCursorRef.current
    if (!cursor) return

    if (isBrushMode && isInsideRef.current && !altPressedRef.current) {
      cursor.style.opacity = '1'
      if (mousePosRef.current && updateCursorPositionRef.current) {
        updateCursorPositionRef.current(mousePosRef.current.x, mousePosRef.current.y)
      }
    } else {
      cursor.style.opacity = '0'
    }
  }, [isBrushMode])

  const isBrushModeRef = useRef(isBrushMode)
  useEffect(() => {
    isBrushModeRef.current = isBrushMode
    syncVisibility()
  }, [isBrushMode, syncVisibility])

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Alt') {
        altPressedRef.current = true
        syncVisibility()
      }
    }

    const handleKeyUp = (e: KeyboardEvent) => {
      if (e.key === 'Alt') {
        altPressedRef.current = false
        syncVisibility()
      }
    }

    const handleBlur = () => {
      altPressedRef.current = false
      syncVisibility()
    }

    window.addEventListener('keydown', handleKeyDown)
    window.addEventListener('keyup', handleKeyUp)
    window.addEventListener('blur', handleBlur)

    return () => {
      window.removeEventListener('keydown', handleKeyDown)
      window.removeEventListener('keyup', handleKeyUp)
      window.removeEventListener('blur', handleBlur)
    }
  }, [isBrushMode, syncVisibility])

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
    updateCursorPositionRef.current = updateCursorPosition

    const handleMove = (e: PointerEvent) => {
      mousePosRef.current = { x: e.clientX, y: e.clientY }
      if (isInsideRef.current) {
        updateCursorPosition(e.clientX, e.clientY)
      }
    }

    const handleEnter = () => {
      isInsideRef.current = true
      refresh()
      syncVisibility()
    }

    const handleLeave = () => {
      isInsideRef.current = false
      syncVisibility()
    }

    const resizeObserver = new ResizeObserver(() => refresh())
    resizeObserver.observe(canvas)

    window.addEventListener('pointermove', handleMove, { capture: true })
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
      window.removeEventListener('pointermove', handleMove, { capture: true })
      canvas.removeEventListener('pointerenter', handleEnter)
      canvas.removeEventListener('pointerleave', handleLeave)
      window.removeEventListener('scroll', refresh, true)
      window.removeEventListener('resize', refresh)
      updateCursorPositionRef.current = null
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [canvasRef, pageKey, syncVisibility])

  return { brushCursorRef, isBrushMode, brushSize }
}
