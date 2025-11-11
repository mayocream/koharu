'use client'

import { useCallback } from 'react'
import { useAppStore } from '@/lib/store'
import { TextBlock } from '@/types'

export function useTextBlocks() {
  const document = useAppStore(
    (state) => state.documents[state.currentDocumentIndex],
  )
  const textBlocks = document?.textBlocks ?? []
  const selectedBlockIndex = useAppStore((state) => state.selectedBlockIndex)
  const setSelectedBlockIndex = useAppStore(
    (state) => state.setSelectedBlockIndex,
  )
  const updateTextBlocks = useAppStore((state) => state.updateTextBlocks)

  const replaceBlock = useCallback(
    async (index: number, updates: Partial<TextBlock>) => {
      const nextBlocks = textBlocks.map((block, idx) =>
        idx === index ? { ...block, ...updates } : block,
      )
      await updateTextBlocks(nextBlocks)
    },
    [textBlocks, updateTextBlocks],
  )

  const appendBlock = useCallback(
    async (block: TextBlock) => {
      const nextBlocks = [...textBlocks, block]
      await updateTextBlocks(nextBlocks)
      setSelectedBlockIndex(nextBlocks.length - 1)
    },
    [setSelectedBlockIndex, textBlocks, updateTextBlocks],
  )

  const removeBlock = useCallback(
    async (index: number) => {
      const nextBlocks = textBlocks.filter((_, idx) => idx !== index)
      await updateTextBlocks(nextBlocks)
      setSelectedBlockIndex(undefined)
    },
    [setSelectedBlockIndex, textBlocks, updateTextBlocks],
  )

  const clearSelection = useCallback(() => {
    setSelectedBlockIndex(undefined)
  }, [setSelectedBlockIndex])

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
