'use client'

import { useQueryClient } from '@tanstack/react-query'
import { useCallback } from 'react'

import {
  useGetDocument,
  getGetDocumentQueryKey,
  getListDocumentsQueryKey,
} from '@/lib/api/documents/documents'
export { useBlobData, useDocumentLayer } from '@/hooks/useBlobData'
import type { DocumentDetail, TextBlockInput } from '@/lib/api/schemas'
import { createTextBlock, patchTextBlock, putTextBlocks } from '@/lib/api/text-blocks/text-blocks'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { TextBlock } from '@/types'

const hasGeometryChange = (updates: Partial<TextBlock>) =>
  Object.prototype.hasOwnProperty.call(updates, 'x') ||
  Object.prototype.hasOwnProperty.call(updates, 'y') ||
  Object.prototype.hasOwnProperty.call(updates, 'width') ||
  Object.prototype.hasOwnProperty.call(updates, 'height')

const mapTextBlock = (block: DocumentDetail['textBlocks'][number]): TextBlock => ({
  id: block.id,
  x: block.x,
  y: block.y,
  width: block.width,
  height: block.height,
  confidence: block.confidence,
  linePolygons: block.linePolygons as TextBlock['linePolygons'],
  sourceDirection: block.sourceDirection ?? undefined,
  renderedDirection: block.renderedDirection ?? undefined,
  sourceLanguage: block.sourceLanguage ?? undefined,
  rotationDeg: block.rotationDeg ?? undefined,
  detectedFontSizePx: block.detectedFontSizePx ?? undefined,
  detector: block.detector ?? undefined,
  text: block.text ?? undefined,
  translation: block.translation ?? undefined,
  style: block.style as TextBlock['style'],
  fontPrediction: block.fontPrediction as TextBlock['fontPrediction'],
  rendered: block.rendered ?? undefined,
  renderX: block.renderX ?? undefined,
  renderY: block.renderY ?? undefined,
  renderWidth: block.renderWidth ?? undefined,
  renderHeight: block.renderHeight ?? undefined,
})

export type MappedDocument = {
  id: string
  name: string
  width: number
  height: number
  textBlocks: TextBlock[]
  /** Blob hashes for each layer — fetch bytes via useDocumentLayer(). */
  image: string
  segment?: string
  inpainted?: string
  brushLayer?: string
  rendered?: string
  style?: { defaultFont?: string | null }
}

const mapDocumentDetail = (detail: DocumentDetail): MappedDocument => ({
  id: detail.id,
  name: detail.name,
  width: detail.width,
  height: detail.height,
  textBlocks: detail.textBlocks.map(mapTextBlock),
  image: detail.image,
  segment: detail.segment ?? undefined,
  inpainted: detail.inpainted ?? undefined,
  brushLayer: detail.brushLayer ?? undefined,
  rendered: detail.rendered ?? undefined,
  style: detail.style ?? undefined,
})

const toTextBlockInput = (block: TextBlock): TextBlockInput => ({
  id: block.id ?? null,
  x: block.x,
  y: block.y,
  width: block.width,
  height: block.height,
  text: block.text ?? null,
  translation: block.translation ?? null,
  style: (block.style as any) ?? null,
})

export function useCurrentDocument(): MappedDocument | null {
  const documentId = useEditorUiStore((s) => s.currentDocumentId)
  const { data: detail } = useGetDocument(documentId ?? '', {
    query: { enabled: !!documentId },
  })
  if (!detail) return null
  return mapDocumentDetail(detail)
}

export function useTextBlocks() {
  const queryClient = useQueryClient()
  const document = useCurrentDocument()
  const textBlocks = document?.textBlocks ?? []
  const selectedBlockIndex = useEditorUiStore((state) => state.selectedBlockIndex)
  const setSelectedBlockIndex = useEditorUiStore((state) => state.setSelectedBlockIndex)

  const invalidateDocument = useCallback(
    async (docId: string) => {
      await queryClient.invalidateQueries({
        queryKey: getGetDocumentQueryKey(docId),
      })
      await queryClient.invalidateQueries({
        queryKey: getListDocumentsQueryKey(),
      })
    },
    [queryClient],
  )

  const updateTextBlocks = useCallback(
    async (blocks: TextBlock[]) => {
      const docId = useEditorUiStore.getState().currentDocumentId
      if (!docId) return
      await putTextBlocks(docId, blocks.map(toTextBlockInput))
      await invalidateDocument(docId)
    },
    [invalidateDocument],
  )

  const replaceBlock = async (index: number, updates: Partial<TextBlock>) => {
    const docId = useEditorUiStore.getState().currentDocumentId
    if (!docId) return
    const block = document?.textBlocks?.[index]
    if (!block?.id) return

    const patch: Record<string, unknown> = {}
    for (const [key, value] of Object.entries(updates)) {
      patch[key] = value
    }

    if (hasGeometryChange(updates)) {
      const ui = useEditorUiStore.getState()
      ui.setShowRenderedImage(false)
      ui.setShowTextBlocksOverlay(true)
    }

    await patchTextBlock(docId, block.id, patch)
    await invalidateDocument(docId)
  }

  const appendBlock = async (block: TextBlock) => {
    const docId = useEditorUiStore.getState().currentDocumentId
    if (!docId) return
    await createTextBlock(docId, {
      x: block.x,
      y: block.y,
      width: block.width,
      height: block.height,
    })
    await invalidateDocument(docId)
    const currentBlocks = document?.textBlocks ?? []
    setSelectedBlockIndex(currentBlocks.length)
  }

  const removeBlock = async (index: number) => {
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
    updateTextBlocks,
  }
}
