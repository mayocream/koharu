'use client'

import { useAppStore } from '@/lib/store'
import { Document, InpaintRegion, TextBlock } from '@/types'

const TEXT_BLOCK_INPAINT_RADIUS = 12

const buildInpaintRegion = (block: TextBlock, doc: Document): InpaintRegion => {
  const x0 = Math.max(0, Math.floor(block.x - TEXT_BLOCK_INPAINT_RADIUS))
  const y0 = Math.max(0, Math.floor(block.y - TEXT_BLOCK_INPAINT_RADIUS))
  const x1 = Math.min(
    doc.width,
    Math.ceil(block.x + block.width + TEXT_BLOCK_INPAINT_RADIUS),
  )
  const y1 = Math.min(
    doc.height,
    Math.ceil(block.y + block.height + TEXT_BLOCK_INPAINT_RADIUS),
  )

  return {
    x: x0,
    y: y0,
    width: Math.max(1, x1 - x0),
    height: Math.max(1, y1 - y0),
  }
}

const pickLargestRegion = (
  a: InpaintRegion,
  b: InpaintRegion,
): InpaintRegion => (a.width * a.height >= b.width * b.height ? a : b)

const shouldRenderSprite = (updates: Partial<TextBlock>) =>
  Object.prototype.hasOwnProperty.call(updates, 'width') ||
  Object.prototype.hasOwnProperty.call(updates, 'height') ||
  Object.prototype.hasOwnProperty.call(updates, 'translation') ||
  Object.prototype.hasOwnProperty.call(updates, 'style')

const shouldInpaint = (updates: Partial<TextBlock>) =>
  Object.prototype.hasOwnProperty.call(updates, 'width') ||
  Object.prototype.hasOwnProperty.call(updates, 'height')

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
  const inpaintPartial = useAppStore((state) => state.inpaintPartial)

  const replaceBlock = async (index: number, updates: Partial<TextBlock>) => {
    const { documents, currentDocumentIndex } = useAppStore.getState()
    const currentBlocks = documents[currentDocumentIndex]?.textBlocks ?? []
    const nextBlocks = currentBlocks.map((block, idx) =>
      idx === index ? { ...block, ...updates } : block,
    )
    await updateTextBlocks(nextBlocks)

    const doc = documents[currentDocumentIndex]

    if (shouldRenderSprite(updates)) {
      void renderTextBlock(undefined, currentDocumentIndex, index)
    }

    if (doc?.segment && shouldInpaint(updates)) {
      const prevBlock = currentBlocks[index]
      const nextBlock = nextBlocks[index]
      const region = prevBlock
        ? pickLargestRegion(
            buildInpaintRegion(prevBlock, doc),
            buildInpaintRegion(nextBlock, doc),
          )
        : buildInpaintRegion(nextBlock, doc)
      console.log('Inpainting region for text block update:', region)
      void inpaintPartial(region, { index: currentDocumentIndex })
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
