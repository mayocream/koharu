'use client'

import { useEffect, useRef, useCallback } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { useCurrentDocumentState } from '@/lib/query/hooks'
import { useTextBlockMutations } from '@/lib/query/mutations'
import { createTempTextBlockId } from '@/lib/api'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { useUndoStore } from '@/lib/stores/undoStore'
import { queryKeys } from '@/lib/query/keys'
import { TextBlock, Document } from '@/types'
import { enqueueTextBlockSync } from '@/lib/services/syncQueues'

const TEXT_BLOCK_RENDER_DEBOUNCE_MS = 250
const POSITION_SYNC_DEBOUNCE_MS = 400

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

const isPositionOnlyChange = (updates: Partial<TextBlock>) => {
  const keys = Object.keys(updates)
  return keys.length > 0 && keys.every((k) => k === 'x' || k === 'y')
}

export function useTextBlocks() {
  const queryClient = useQueryClient()
  const { currentDocument: document, currentDocumentIndex } =
    useCurrentDocumentState()
  const textBlocks = document?.textBlocks ?? []
  const selectedBlockIndex = useEditorUiStore(
    (state) => state.selectedBlockIndex,
  )
  const selectedBlockIndices = useEditorUiStore(
    (state) => state.selectedBlockIndices,
  )
  const setSelectedBlockIndex = useEditorUiStore(
    (state) => state.setSelectedBlockIndex,
  )
  const toggleBlockSelection = useEditorUiStore(
    (state) => state.toggleBlockSelection,
  )
  const clearBlockSelection = useEditorUiStore(
    (state) => state.clearBlockSelection,
  )
  const { updateTextBlocks, renderTextBlock } = useTextBlockMutations()
  const pushUndo = useUndoStore((state) => state.push)
  const renderTimersRef = useRef<Map<number, ReturnType<typeof setTimeout>>>(
    new Map(),
  )
  const positionSyncTimerRef = useRef<ReturnType<typeof setTimeout> | null>(
    null,
  )
  // Track the block state at drag-start to create a single undo entry per drag session
  const dragUndoRef = useRef<{
    index: number
    snapshot: TextBlock
  } | null>(null)

  useEffect(() => {
    const timers = renderTimersRef.current
    return () => {
      timers.forEach((timer) => clearTimeout(timer))
      timers.clear()
      if (positionSyncTimerRef.current) {
        clearTimeout(positionSyncTimerRef.current)
      }
    }
  }, [])

  /** Read the latest textBlocks directly from React Query cache (never stale). */
  const readCurrentBlocks = useCallback((): TextBlock[] => {
    const idx = useEditorUiStore.getState().currentDocumentIndex
    const doc = queryClient.getQueryData<Document>(
      queryKeys.documents.current(idx),
    )
    return doc?.textBlocks ?? []
  }, [queryClient])

  /** Optimistically update textBlocks in the React Query cache without hitting the backend. */
  const setCacheBlocks = useCallback(
    (nextBlocks: TextBlock[]) => {
      const idx = useEditorUiStore.getState().currentDocumentIndex
      const queryKey = queryKeys.documents.current(idx)
      void queryClient.cancelQueries({ queryKey })
      const doc = queryClient.getQueryData<any>(queryKey)
      if (!doc) return
      queryClient.setQueryData(queryKey, { ...doc, textBlocks: nextBlocks })
    },
    [queryClient],
  )

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
    const currentBlocks = readCurrentBlocks()
    const oldBlock = currentBlocks[index]
    if (!oldBlock) return
    const nextBlocks = currentBlocks.map((block, idx) =>
      idx === index ? { ...block, ...updates } : block,
    )

    if (isPositionOnlyChange(updates)) {
      // FIX: Update cache immediately so the UI shows the new position at once.
      // Only debounce the backend sync.
      setCacheBlocks(nextBlocks)

      if (positionSyncTimerRef.current) {
        clearTimeout(positionSyncTimerRef.current)
      }
      positionSyncTimerRef.current = setTimeout(() => {
        positionSyncTimerRef.current = null
        // Read the LATEST cache (user may have moved more since this was scheduled)
        const latestBlocks = readCurrentBlocks()
        const idx = useEditorUiStore.getState().currentDocumentIndex
        void enqueueTextBlockSync(idx, latestBlocks)
      }, POSITION_SYNC_DEBOUNCE_MS)

      // Undo for position: capture snapshot at drag-start only (not every intermediate move)
      if (!dragUndoRef.current || dragUndoRef.current.index !== index) {
        dragUndoRef.current = { index, snapshot: { ...oldBlock } }
      }
    } else {
      // Non-position changes: update cache + sync immediately
      await updateTextBlocks(nextBlocks)

      // Push undo (non-position changes)
      const snapshot = { ...oldBlock }
      const applied = { ...updates }
      pushUndo({
        type: 'replaceBlock',
        description: `Edit block ${index + 1}`,
        undo: () => {
          const blocks = readCurrentBlocks()
          const restored = blocks.map((b, idx) =>
            idx === index ? snapshot : b,
          )
          void updateTextBlocks(restored)
        },
        redo: () => {
          const blocks = readCurrentBlocks()
          const reapplied = blocks.map((b, idx) =>
            idx === index ? { ...b, ...applied } : b,
          )
          void updateTextBlocks(reapplied)
        },
      })
    }

    if (hasGeometryChange(updates)) {
      const ui = useEditorUiStore.getState()
      ui.setShowRenderedImage(false)
      ui.setShowTextBlocksOverlay(true)
    }

    if (shouldRenderSprite(updates)) {
      if (shouldRenderSpriteImmediately(updates)) {
        clearScheduledRender(index)
        void renderTextBlock(undefined, currentDocumentIndex, index)
      } else {
        scheduleRender(index)
      }
    }
  }

  /** Call after a drag session ends to commit the undo entry for the whole drag. */
  const commitDragUndo = useCallback(() => {
    const entry = dragUndoRef.current
    if (!entry) return
    dragUndoRef.current = null

    const { index, snapshot } = entry
    const finalBlock = readCurrentBlocks()[index]
    if (!finalBlock) return
    // Don't push if position didn't actually change
    if (finalBlock.x === snapshot.x && finalBlock.y === snapshot.y) return

    const finalPos = { x: finalBlock.x, y: finalBlock.y }
    pushUndo({
      type: 'moveBlock',
      description: `Move block ${index + 1}`,
      undo: () => {
        const blocks = readCurrentBlocks()
        const restored = blocks.map((b, idx) =>
          idx === index ? { ...b, x: snapshot.x, y: snapshot.y } : b,
        )
        void updateTextBlocks(restored)
      },
      redo: () => {
        const blocks = readCurrentBlocks()
        const reapplied = blocks.map((b, idx) =>
          idx === index ? { ...b, x: finalPos.x, y: finalPos.y } : b,
        )
        void updateTextBlocks(reapplied)
      },
    })
  }, [pushUndo, readCurrentBlocks, updateTextBlocks])

  const replaceMultipleBlocks = async (
    indices: number[],
    updates: Partial<TextBlock>,
  ) => {
    const currentBlocks = readCurrentBlocks()
    const indexSet = new Set(indices)
    const snapshots = new Map<number, TextBlock>()
    indices.forEach((i) => {
      if (currentBlocks[i]) snapshots.set(i, { ...currentBlocks[i] })
    })

    const nextBlocks = currentBlocks.map((block, idx) =>
      indexSet.has(idx) ? { ...block, ...updates } : block,
    )
    await updateTextBlocks(nextBlocks)

    pushUndo({
      type: 'replaceMultipleBlocks',
      description: `Edit ${indices.length} blocks`,
      undo: () => {
        const blocks = readCurrentBlocks()
        const restored = blocks.map((b, idx) => snapshots.get(idx) ?? b)
        void updateTextBlocks(restored)
      },
      redo: () => {
        const blocks = readCurrentBlocks()
        const reapplied = blocks.map((b, idx) =>
          indexSet.has(idx) ? { ...b, ...updates } : b,
        )
        void updateTextBlocks(reapplied)
      },
    })

    if (hasGeometryChange(updates)) {
      const ui = useEditorUiStore.getState()
      ui.setShowRenderedImage(false)
      ui.setShowTextBlocksOverlay(true)
    }

    if (shouldRenderSprite(updates)) {
      for (const idx of indices) {
        if (shouldRenderSpriteImmediately(updates)) {
          clearScheduledRender(idx)
          void renderTextBlock(undefined, currentDocumentIndex, idx)
        } else {
          scheduleRender(idx)
        }
      }
    }
  }

  const appendBlock = async (block: TextBlock) => {
    const currentBlocks = readCurrentBlocks()
    const newBlock = {
      ...block,
      id: block.id ?? createTempTextBlockId(),
    }
    const nextBlocks = [...currentBlocks, newBlock]
    await updateTextBlocks(nextBlocks)
    const addedIndex = nextBlocks.length - 1
    setSelectedBlockIndex(addedIndex)

    const blockCopy = { ...newBlock }
    pushUndo({
      type: 'appendBlock',
      description: `Add block ${addedIndex + 1}`,
      undo: () => {
        const blocks = readCurrentBlocks()
        void updateTextBlocks(blocks.slice(0, -1))
        setSelectedBlockIndex(undefined)
      },
      redo: () => {
        const blocks = readCurrentBlocks()
        void updateTextBlocks([...blocks, blockCopy])
        setSelectedBlockIndex(blocks.length)
      },
    })
  }

  const removeBlock = async (index: number) => {
    clearScheduledRender(index)
    const currentBlocks = readCurrentBlocks()
    const removedBlock = currentBlocks[index]
    const nextBlocks = currentBlocks.filter((_, idx) => idx !== index)
    await updateTextBlocks(nextBlocks)
    setSelectedBlockIndex(undefined)

    if (removedBlock) {
      const blockCopy = { ...removedBlock }
      pushUndo({
        type: 'removeBlock',
        description: `Delete block ${index + 1}`,
        undo: () => {
          const blocks = readCurrentBlocks()
          const restored = [
            ...blocks.slice(0, index),
            blockCopy,
            ...blocks.slice(index),
          ]
          void updateTextBlocks(restored)
          setSelectedBlockIndex(index)
        },
        redo: () => {
          const blocks = readCurrentBlocks()
          void updateTextBlocks(blocks.filter((_, idx) => idx !== index))
          setSelectedBlockIndex(undefined)
        },
      })
    }
  }

  const clearSelection = useCallback(() => {
    clearBlockSelection()
  }, [clearBlockSelection])

  return {
    document,
    textBlocks,
    selectedBlockIndex,
    selectedBlockIndices,
    setSelectedBlockIndex,
    toggleBlockSelection,
    clearSelection,
    replaceBlock,
    replaceMultipleBlocks,
    appendBlock,
    removeBlock,
    commitDragUndo,
  }
}
