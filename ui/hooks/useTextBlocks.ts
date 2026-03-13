'use client'

import { useEffect, useRef } from 'react'
import { useCurrentDocumentState } from '@/lib/query/hooks'
import { useTextBlockMutations } from '@/lib/query/mutations'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { TextBlock } from '@/types'

const TEXT_BLOCK_RENDER_DEBOUNCE_MS = 250

const shouldRenderSprite = (updates: Partial<TextBlock>) =>
  Object.prototype.hasOwnProperty.call(updates, 'width') ||
  Object.prototype.hasOwnProperty.call(updates, 'height') ||
  Object.prototype.hasOwnProperty.call(updates, 'translation') ||
  Object.prototype.hasOwnProperty.call(updates, 'style')

const shouldRenderSpriteImmediately = (updates: Partial<TextBlock>) =>
  Object.prototype.hasOwnProperty.call(updates, 'width') ||
  Object.prototype.hasOwnProperty.call(updates, 'height')

const hasGeometryChange = (updates: Partial<TextBlock>) =>
  Object.prototype.hasOwnProperty.call(updates, 'x') ||
  Object.prototype.hasOwnProperty.call(updates, 'y') ||
  Object.prototype.hasOwnProperty.call(updates, 'width') ||
  Object.prototype.hasOwnProperty.call(updates, 'height')

export function useTextBlocks() {
  const { currentDocument: document, currentDocumentIndex } =
    useCurrentDocumentState()
  const textBlocks = document?.textBlocks ?? []
  const selectedBlockIndex = useEditorUiStore(
    (state) => state.selectedBlockIndex,
  )
  const setSelectedBlockIndex = useEditorUiStore(
    (state) => state.setSelectedBlockIndex,
  )
  const { updateTextBlocks, renderTextBlock } = useTextBlockMutations()
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
      void renderTextBlock(undefined, currentDocumentIndex, index)
    }, TEXT_BLOCK_RENDER_DEBOUNCE_MS)
    renderTimersRef.current.set(index, timer)
  }

  const replaceBlock = async (index: number, updates: Partial<TextBlock>) => {
    const currentBlocks = document?.textBlocks ?? []
    const nextBlocks = currentBlocks.map((block, idx) =>
      idx === index ? { ...block, ...updates } : block,
    )
    await updateTextBlocks(nextBlocks)

    if (hasGeometryChange(updates)) {
      const ui = useEditorUiStore.getState()
      ui.setShowRenderedImage(false)
      ui.setShowTextBlocksOverlay(true)
    }

    const doc = document

    if (shouldRenderSprite(updates)) {
      if (shouldRenderSpriteImmediately(updates)) {
        clearScheduledRender(index)
        void renderTextBlock(undefined, currentDocumentIndex, index)
      } else {
        scheduleRender(index)
      }
    }
  }

  const appendBlock = async (block: TextBlock) => {
    const currentBlocks = document?.textBlocks ?? []
    const nextBlocks = [...currentBlocks, block]
    await updateTextBlocks(nextBlocks)
    setSelectedBlockIndex(nextBlocks.length - 1)
  }

  const removeBlock = async (index: number) => {
    clearScheduledRender(index)
    const currentBlocks = document?.textBlocks ?? []
    const nextBlocks = currentBlocks.filter((_, idx) => idx !== index)
    await updateTextBlocks(nextBlocks)
    setSelectedBlockIndex(undefined)
  }

  const clearSelection = () => {
    setSelectedBlockIndex(undefined)
  }

  return {
    document,
    textBlocks,
    selectedBlockIndex,
    setSelectedBlockIndex,
    clearSelection,
    replaceBlock,
    appendBlock,
    removeBlock,
  }
}
