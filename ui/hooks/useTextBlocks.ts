'use client'

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

  const replaceBlock = async (index: number, updates: Partial<TextBlock>) => {
    const nextBlocks = textBlocks.map((block, idx) =>
      idx === index ? { ...block, ...updates } : block,
    )
    await updateTextBlocks(nextBlocks)
  }

  const appendBlock = async (block: TextBlock) => {
    const nextBlocks = [...textBlocks, block]
    await updateTextBlocks(nextBlocks)
    setSelectedBlockIndex(nextBlocks.length - 1)
  }

  const removeBlock = async (index: number) => {
    const nextBlocks = textBlocks.filter((_, idx) => idx !== index)
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
