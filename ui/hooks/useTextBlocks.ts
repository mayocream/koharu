'use client'

import { useRef, useCallback } from 'react'
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

// Module-level singletons so all useTextBlocks() instances share the same
// debounce timers.  Without this, each component that calls the hook gets its
// own timer refs – two components editing the same block fire two render
// requests instead of one.
const sharedRenderTimers = new Map<number, ReturnType<typeof setTimeout>>()
let sharedPositionSyncTimer: ReturnType<typeof setTimeout> | null = null

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
  // Track the block state at drag-start to create a single undo entry per drag session
  const dragUndoRef = useRef<{
    index: number
    snapshot: TextBlock
  } | null>(null)

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
    const timer = sharedRenderTimers.get(index)
    if (!timer) return
    clearTimeout(timer)
    sharedRenderTimers.delete(index)
  }

  const scheduleRender = (index: number) => {
    clearScheduledRender(index)
    const timer = setTimeout(() => {
      sharedRenderTimers.delete(index)
      // Read currentDocumentIndex from the store at fire time so a page
      // change during the debounce window doesn't render on the old page.
      const docIdx = useEditorUiStore.getState().currentDocumentIndex
      void renderTextBlock(undefined, docIdx, index)
    }, TEXT_BLOCK_RENDER_DEBOUNCE_MS)
    sharedRenderTimers.set(index, timer)
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

      if (sharedPositionSyncTimer) {
        clearTimeout(sharedPositionSyncTimer)
      }
      sharedPositionSyncTimer = setTimeout(() => {
        sharedPositionSyncTimer = null
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
      const docIdx = useEditorUiStore.getState().currentDocumentIndex
      if (shouldRenderSpriteImmediately(updates)) {
        clearScheduledRender(index)
        void renderTextBlock(undefined, docIdx, index)
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
      const docIdx = useEditorUiStore.getState().currentDocumentIndex
      for (const idx of indices) {
        if (shouldRenderSpriteImmediately(updates)) {
          clearScheduledRender(idx)
          void renderTextBlock(undefined, docIdx, idx)
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

  const mergeBlocks = async (indices: number[]) => {
    if (indices.length < 2) return
    const currentBlocks = readCurrentBlocks()
    const toMerge = indices
      .map((i) => ({ index: i, block: currentBlocks[i] }))
      .filter((entry) => !!entry.block)
    if (toMerge.length < 2) return

    // Sort by Y coordinate (top to bottom) for text concatenation order
    const sorted = [...toMerge].sort((a, b) => a.block.y - b.block.y)

    // Union bounding box
    let minX = Infinity
    let minY = Infinity
    let maxX = -Infinity
    let maxY = -Infinity
    for (const { block } of sorted) {
      minX = Math.min(minX, block.x)
      minY = Math.min(minY, block.y)
      maxX = Math.max(maxX, block.x + block.width)
      maxY = Math.max(maxY, block.y + block.height)
    }

    // Find the largest block (by area) for style/font/direction
    const largest = toMerge.reduce((best, curr) => {
      const bestArea = best.block.width * best.block.height
      const currArea = curr.block.width * curr.block.height
      return currArea > bestArea ? curr : best
    })

    // Concatenate text and translation in Y order, separated by newline
    const mergedText = sorted
      .map((e) => e.block.text?.trim())
      .filter(Boolean)
      .join('\n')
    const mergedTranslation = sorted
      .map((e) => e.block.translation?.trim())
      .filter(Boolean)
      .join('\n')

    const mergedBlock: TextBlock = {
      id: createTempTextBlockId(),
      x: Math.round(minX),
      y: Math.round(minY),
      width: Math.round(maxX - minX),
      height: Math.round(maxY - minY),
      confidence: Math.max(...toMerge.map((e) => e.block.confidence)),
      sourceDirection: largest.block.sourceDirection,
      renderedDirection: largest.block.renderedDirection,
      sourceLanguage: largest.block.sourceLanguage,
      detectedFontSizePx: largest.block.detectedFontSizePx,
      detector: largest.block.detector,
      text: mergedText || undefined,
      translation: mergedTranslation || undefined,
      style: largest.block.style,
      fontPrediction: largest.block.fontPrediction,
    }

    // Remove merged blocks and insert the new one at the position of the
    // first (topmost) merged block so the numbering stays intuitive.
    const removeSet = new Set(indices)
    const nextBlocks: TextBlock[] = []
    let inserted = false
    for (let i = 0; i < currentBlocks.length; i++) {
      if (removeSet.has(i)) {
        clearScheduledRender(i)
        if (!inserted) {
          nextBlocks.push(mergedBlock)
          inserted = true
        }
        continue
      }
      nextBlocks.push(currentBlocks[i])
    }
    if (!inserted) nextBlocks.push(mergedBlock)

    const mergedIndex = nextBlocks.indexOf(mergedBlock)

    // Push undo BEFORE the async updateTextBlocks so Ctrl+Z is available
    // immediately, not after the backend sync completes.
    const snapshotBlocks = currentBlocks.map((b) => ({ ...b }))
    const mergedBlockCopy = { ...mergedBlock }
    pushUndo({
      type: 'mergeBlocks',
      description: `Merge ${indices.length} blocks`,
      undo: () => {
        void updateTextBlocks(snapshotBlocks)
        clearBlockSelection()
      },
      redo: () => {
        const redoBlocks: TextBlock[] = []
        const set = new Set(indices)
        let ins = false
        for (const [i, b] of snapshotBlocks.entries()) {
          if (set.has(i)) {
            if (!ins) {
              redoBlocks.push(mergedBlockCopy)
              ins = true
            }
            continue
          }
          redoBlocks.push(b)
        }
        if (!ins) redoBlocks.push(mergedBlockCopy)
        void updateTextBlocks(redoBlocks)
        const idx = redoBlocks.indexOf(mergedBlockCopy)
        setSelectedBlockIndex(idx >= 0 ? idx : undefined)
      },
    })

    // Clear stale multi-select indices BEFORE updating blocks so that a
    // React re-render during the async sync doesn't display old indices
    // pointing at wrong blocks in the new array.
    clearBlockSelection()
    await updateTextBlocks(nextBlocks)
    setSelectedBlockIndex(mergedIndex)

    // Show text block overlay and hide rendered image since geometry changed
    const ui = useEditorUiStore.getState()
    ui.setShowRenderedImage(false)
    ui.setShowTextBlocksOverlay(true)

    // Trigger render for the new merged block
    const docIdx = useEditorUiStore.getState().currentDocumentIndex
    void renderTextBlock(undefined, docIdx, mergedIndex)
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
    readCurrentBlocks,
    mergeBlocks,
    appendBlock,
    removeBlock,
    commitDragUndo,
  }
}
