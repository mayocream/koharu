'use client'

import { useEffect, useRef } from 'react'

const TEXT_BLOCK_RENDER_DEBOUNCE_MS = 250

export const useTextBlockRenderScheduler = (
  renderTextBlock: (
    _?: unknown,
    documentId?: string,
    textBlockIndex?: number,
  ) => Promise<void>,
  currentDocumentId?: string,
) => {
  const renderTimersRef = useRef<Map<number, ReturnType<typeof setTimeout>>>(
    new Map(),
  )

  useEffect(() => {
    const timers = renderTimersRef.current
    return () => {
      timers.forEach((timer) => clearTimeout(timer))
      timers.clear()
    }
  }, [])

  const clearScheduledRender = (index: number) => {
    const timer = renderTimersRef.current.get(index)
    if (!timer) return
    clearTimeout(timer)
    renderTimersRef.current.delete(index)
  }

  const scheduleRender = (index: number) => {
    clearScheduledRender(index)
    const timer = setTimeout(() => {
      renderTimersRef.current.delete(index)
      void renderTextBlock(undefined, currentDocumentId, index)
    }, TEXT_BLOCK_RENDER_DEBOUNCE_MS)
    renderTimersRef.current.set(index, timer)
  }

  return {
    clearScheduledRender,
    scheduleRender,
  }
}
