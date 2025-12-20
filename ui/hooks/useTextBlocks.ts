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
  const renderTextBlock = useAppStore((state) => state.renderTextBlock)

  const shouldRenderSprite = (updates: Partial<TextBlock>) =>
    Object.prototype.hasOwnProperty.call(updates, 'width') ||
    Object.prototype.hasOwnProperty.call(updates, 'height') ||
    Object.prototype.hasOwnProperty.call(updates, 'translation') ||
    Object.prototype.hasOwnProperty.call(updates, 'style')

  const replaceBlock = async (index: number, updates: Partial<TextBlock>) => {
    const { documents, currentDocumentIndex } = useAppStore.getState()
    const currentBlocks = documents[currentDocumentIndex]?.textBlocks ?? []
    const nextBlocks = currentBlocks.map((block, idx) =>
      idx === index ? { ...block, ...updates } : block,
    )
    await updateTextBlocks(nextBlocks)
    if (shouldRenderSprite(updates)) {
      void renderTextBlock(undefined, currentDocumentIndex, index)
    }
  }

  const appendBlock = async (block: TextBlock) => {
    const { documents, currentDocumentIndex } = useAppStore.getState()
    const currentBlocks = documents[currentDocumentIndex]?.textBlocks ?? []
    const nextBlocks = [...currentBlocks, block]
    await updateTextBlocks(nextBlocks)
    setSelectedBlockIndex(nextBlocks.length - 1)
  }

  const removeBlock = async (index: number) => {
    const { documents, currentDocumentIndex } = useAppStore.getState()
    const currentBlocks = documents[currentDocumentIndex]?.textBlocks ?? []
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
