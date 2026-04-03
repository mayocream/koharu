'use client'

import { useCallback, useEffect, useRef } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import {
  useGetDocument,
  getGetDocumentQueryKey,
  getListDocumentsQueryKey,
} from '@/lib/api/documents/documents'
import { putTextBlocks } from '@/lib/api/text-blocks/text-blocks'
import { renderDocument } from '@/lib/api/processing/processing'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'
import { TextBlock } from '@/types'
import type { DocumentDetail, TextBlockInput } from '@/lib/api/schemas'

const createTempTextBlockId = () =>
  `temp:${globalThis.crypto?.randomUUID?.() ?? Math.random().toString(36).slice(2)}`

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

const toUint8Array = (
  data: number[] | null | undefined,
): Uint8Array | undefined => (data ? new Uint8Array(data) : undefined)

const mapTextBlock = (
  block: DocumentDetail['textBlocks'][number],
): TextBlock => ({
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
  rendered: undefined,
})

export type MappedDocument = {
  id: string
  name: string
  width: number
  height: number
  textBlocks: TextBlock[]
  image: Uint8Array
  segment?: Uint8Array
  inpainted?: Uint8Array
  brushLayer?: Uint8Array
  rendered?: Uint8Array
}

const mapDocumentDetail = (detail: DocumentDetail): MappedDocument => ({
  id: detail.id,
  name: detail.name,
  width: detail.width,
  height: detail.height,
  textBlocks: detail.textBlocks.map(mapTextBlock),
  image: new Uint8Array(detail.image),
  segment: toUint8Array(detail.segment),
  inpainted: toUint8Array(detail.inpainted),
  brushLayer: toUint8Array(detail.brushLayer),
  rendered: toUint8Array(detail.rendered),
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
    query: { enabled: !!documentId, structuralSharing: false },
  })
  if (!detail) return null
  return mapDocumentDetail(detail)
}

export function useTextBlocks() {
  const queryClient = useQueryClient()
  const document = useCurrentDocument()
  const documentId = useEditorUiStore((s) => s.currentDocumentId)
  const textBlocks = document?.textBlocks ?? []
  const selectedBlockIndex = useEditorUiStore(
    (state) => state.selectedBlockIndex,
  )
  const setSelectedBlockIndex = useEditorUiStore(
    (state) => state.setSelectedBlockIndex,
  )
  const renderTimersRef = useRef<Map<number, ReturnType<typeof setTimeout>>>(
    new Map(),
  )

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

  const renderTextBlock = useCallback(
    async (docId: string, textBlockIndex: number) => {
      if (typeof textBlockIndex !== 'number') return
      const { renderEffect, renderStroke } = useEditorUiStore.getState()
      const { fontFamily } = usePreferencesStore.getState()
      await renderDocument(docId, {
        shaderEffect: renderEffect,
        shaderStroke: renderStroke,
        fontFamily,
      })
      await invalidateDocument(docId)
    },
    [invalidateDocument],
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
      void renderTextBlock(documentId!, index)
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

    if (shouldRenderSprite(updates)) {
      if (shouldRenderSpriteImmediately(updates)) {
        clearScheduledRender(index)
        void renderTextBlock(documentId!, index)
      } else {
        scheduleRender(index)
      }
    }
  }

  const appendBlock = async (block: TextBlock) => {
    const currentBlocks = document?.textBlocks ?? []
    const nextBlocks = [
      ...currentBlocks,
      {
        ...block,
        id: block.id ?? createTempTextBlockId(),
      },
    ]
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
    updateTextBlocks,
  }
}
